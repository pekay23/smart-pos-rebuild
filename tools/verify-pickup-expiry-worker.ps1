$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$containerName = "advanced-pos-pickup-expiry-check-$PID"
$runnerPassword = "expiry-$([guid]::NewGuid().ToString('N'))"
$preserveContainer = $env:POS_API_EXPIRY_VERIFY_KEEP_CONTAINER -eq '1'

function Invoke-DockerCommand {
    param([Parameter(Mandatory)][string[]]$Arguments)
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = (Get-Command docker -ErrorAction Stop).Source
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) { [void]$startInfo.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    [void]$process.Start()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(120000)) {
        try { $process.Kill($true) } catch { }
        [void]$process.WaitForExit()
        throw "Docker command timed out: docker $($Arguments -join ' ')"
    }
    [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output = $stdout.GetAwaiter().GetResult()
        Error = $stderr.GetAwaiter().GetResult()
    }
}

function Invoke-RunnerQuery([string]$Sql) {
    Invoke-DockerCommand -Arguments @(
        'exec', '-e', "PGPASSWORD=$runnerPassword", $containerName,
        'psql', '-h', '127.0.0.1', '-U', 'pos_api_expiry_runner',
        '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1', '-At', '-c', $Sql
    )
}

try {
    $start = Invoke-DockerCommand -Arguments @(
        'run', '--rm', '-d', '--name', $containerName,
        '-e', 'POSTGRES_DB=advanced_pos', '-e', 'POSTGRES_USER=pos_migrator',
        '-e', 'POSTGRES_PASSWORD=verification-only', 'postgres:16-alpine'
    )
    if ($start.ExitCode -ne 0) { throw "Could not start disposable PostgreSQL: $($start.Error)" }

    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $probe = Invoke-DockerCommand -Arguments @('exec', $containerName, 'pg_isready', '-U', 'pos_migrator', '-d', 'advanced_pos')
        if ($probe.ExitCode -eq 0) { $ready = $true; break }
        Start-Sleep -Seconds 1
    }
    if (-not $ready) { throw 'Disposable PostgreSQL did not become ready.' }

    $roles = Invoke-DockerCommand -Arguments @(
        'exec', $containerName, 'psql', '-U', 'pos_migrator', '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1',
        '-c', "CREATE ROLE pos_api_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_operator_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_operator_provisioner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_api_expiry_worker NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_api_expiry_runner LOGIN PASSWORD '$runnerPassword' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; GRANT CONNECT ON DATABASE advanced_pos TO pos_api_expiry_runner;"
    )
    if ($roles.ExitCode -ne 0) { throw "Could not provision test database roles: $($roles.Error)" }

    $copy = Invoke-DockerCommand -Arguments @('cp', (Join-Path $repoRoot 'apps/api/migrations/postgres'), "${containerName}:/tmp/migrations")
    if ($copy.ExitCode -ne 0) { throw "Could not copy migrations: $($copy.Error)" }
    $migrationFiles = Get-ChildItem -LiteralPath (Join-Path $repoRoot 'apps/api/migrations/postgres') -Filter '*.sql' -File |
        Where-Object { $_.Name -match '^\d{4}_.+\.sql$' } | Sort-Object Name
    $migrationArgs = @('exec', $containerName, 'psql', '-U', 'pos_migrator', '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1')
    foreach ($file in $migrationFiles) { $migrationArgs += @('-f', "/tmp/migrations/$($file.Name)") }
    $migrations = Invoke-DockerCommand -Arguments $migrationArgs
    if ($migrations.ExitCode -ne 0) { throw "PostgreSQL migration application failed: $($migrations.Error) $($migrations.Output)" }

    $directTableRead = Invoke-RunnerQuery -Sql 'SELECT count(*) FROM public.pickup_reservations;'
    if ($directTableRead.ExitCode -eq 0) { throw 'The expiry runner unexpectedly read pickup reservation rows directly.' }
    $directWorkerCall = Invoke-RunnerQuery -Sql 'SELECT pos_worker.materialize_next_expired_pickup_reservation();'
    if ($directWorkerCall.ExitCode -eq 0) { throw 'The expiry runner unexpectedly invoked the internal materializer directly.' }

    $seedSql = @'
BEGIN;
INSERT INTO tenants(id,display_name) VALUES ('10000000-0000-0000-0000-000000000001','Expiry verification tenant');
INSERT INTO person_identities(id,issuer,subject) VALUES ('30000000-0000-0000-0000-000000000003','https://expiry.example','expiry-subject');
INSERT INTO memberships(id,tenant_id,identity_id) VALUES ('40000000-0000-0000-0000-000000000004','10000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000003');
INSERT INTO branches(id,tenant_id,code,name,timezone) VALUES
 ('50000000-0000-0000-0000-000000000005','10000000-0000-0000-0000-000000000001','ORIGIN','Origin branch','Africa/Accra'),
 ('60000000-0000-0000-0000-000000000006','10000000-0000-0000-0000-000000000001','PICKUP','Pickup branch','Africa/Accra');
INSERT INTO stations(id,tenant_id,branch_id,code,name) VALUES ('80000000-0000-0000-0000-000000000008','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','ORIGIN-1','Origin station');
INSERT INTO membership_role_grants(tenant_id,membership_id,role_key,branch_id) VALUES ('10000000-0000-0000-0000-000000000001','40000000-0000-0000-0000-000000000004','cashier','50000000-0000-0000-0000-000000000005');
INSERT INTO catalog_products(id,tenant_id,sku,name,created_by_membership_id) VALUES ('a0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','EXPIRY-001','Expiry verification item','40000000-0000-0000-0000-000000000004');
INSERT INTO branch_catalog_items(tenant_id,branch_id,product_id,price_minor,stock_quantity) VALUES ('10000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000006','a0000000-0000-0000-0000-000000000001',100,10);

INSERT INTO pickup_reservations
 (id,tenant_id,origin_branch_id,origin_station_id,pickup_branch_id,requested_by_membership_id,
  contact_name_snapshot,contact_phone_snapshot,contact_consent_at,status,revision,expires_at,
  request_idempotency_key,request_hash,created_at,updated_at)
VALUES
 ('b0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','80000000-0000-0000-0000-000000000008','60000000-0000-0000-0000-000000000006','40000000-0000-0000-0000-000000000004','Due hold','+233200000001',clock_timestamp()-interval '1 hour','held',1,clock_timestamp()-interval '30 minutes','seed-due-hold',decode(repeat('11',32),'hex'),clock_timestamp()-interval '1 hour',clock_timestamp()-interval '1 hour'),
 ('c0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','80000000-0000-0000-0000-000000000008','60000000-0000-0000-0000-000000000006','40000000-0000-0000-0000-000000000004','Future hold','+233200000002',clock_timestamp(),'held',1,clock_timestamp()+interval '30 minutes','seed-future-hold',decode(repeat('22',32),'hex'),clock_timestamp(),clock_timestamp());
INSERT INTO pickup_reservation_lines(tenant_id,reservation_id,product_id,pickup_branch_id,sku_snapshot,name_snapshot,quantity) VALUES
 ('10000000-0000-0000-0000-000000000001','b0000000-0000-0000-0000-000000000001','a0000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000006','EXPIRY-001','Expiry verification item',1),
 ('10000000-0000-0000-0000-000000000001','c0000000-0000-0000-0000-000000000001','a0000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000006','EXPIRY-001','Expiry verification item',1);
INSERT INTO pickup_reservation_events(tenant_id,reservation_id,acting_branch_id,acting_station_id,actor_membership_id,event_type,revision,idempotency_key,request_hash) VALUES
 ('10000000-0000-0000-0000-000000000001','b0000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','80000000-0000-0000-0000-000000000008','40000000-0000-0000-0000-000000000004','held',1,'seed-due-held-event',decode(repeat('11',32),'hex')),
 ('10000000-0000-0000-0000-000000000001','c0000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','80000000-0000-0000-0000-000000000008','40000000-0000-0000-0000-000000000004','held',1,'seed-future-held-event',decode(repeat('22',32),'hex'));
INSERT INTO inventory_allocations(tenant_id,branch_id,product_id,pickup_reservation_id,quantity,expires_at) VALUES
 ('10000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000006','a0000000-0000-0000-0000-000000000001','b0000000-0000-0000-0000-000000000001',1,clock_timestamp()-interval '30 minutes'),
 ('10000000-0000-0000-0000-000000000001','60000000-0000-0000-0000-000000000006','a0000000-0000-0000-0000-000000000001','c0000000-0000-0000-0000-000000000001',1,clock_timestamp()+interval '30 minutes');
DO $seed_check$
BEGIN
 IF (SELECT COALESCE(SUM(quantity),0) FROM inventory_allocations WHERE tenant_id='10000000-0000-0000-0000-000000000001' AND branch_id='60000000-0000-0000-0000-000000000006' AND product_id='a0000000-0000-0000-0000-000000000001' AND released_at IS NULL AND (expires_at IS NULL OR expires_at>clock_timestamp())) <> 1 THEN
   RAISE EXCEPTION 'Deadline-based ATP did not exclude the overdue allocation before materialization.';
 END IF;
END
$seed_check$;
COMMIT;
'@
    $seedFile = Join-Path $env:TEMP "pickup-expiry-seed-$PID.sql"
    $seedSql | Set-Content -LiteralPath $seedFile -Encoding utf8
    $seedCopy = Invoke-DockerCommand -Arguments @('cp', $seedFile, "${containerName}:/tmp/seed.sql")
    if ($seedCopy.ExitCode -ne 0) { throw "Could not copy fixture SQL: $($seedCopy.Error)" }
    $seed = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'pos_migrator', '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1', '-f', '/tmp/seed.sql')
    if ($seed.ExitCode -ne 0) { throw "Could not seed pickup expiry fixture: $($seed.Error) $($seed.Output)" }

    $routineSql = 'SELECT pos_worker.materialize_expired_pickup_reservation();'
    $runnerArguments = @('exec', '-e', "PGPASSWORD=$runnerPassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_api_expiry_runner', '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1', '-At', '-c', $routineSql)
    $invocations = @()
    for ($index = 0; $index -lt 2; $index++) {
        $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = (Get-Command docker -ErrorAction Stop).Source
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true
        foreach ($argument in $runnerArguments) { [void]$startInfo.ArgumentList.Add($argument) }
        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $startInfo
        [void]$process.Start()
        $invocations += [pscustomobject]@{
            Process = $process
            Stdout = $process.StandardOutput.ReadToEndAsync()
            Stderr = $process.StandardError.ReadToEndAsync()
        }
    }
    foreach ($invocation in $invocations) {
        if (-not $invocation.Process.WaitForExit(30000)) { try { $invocation.Process.Kill($true) } catch {}; throw 'Concurrent expiry runner call timed out.' }
    }
    $results = @($invocations | ForEach-Object {
        $out = $_.Stdout.GetAwaiter().GetResult().Trim()
        $err = $_.Stderr.GetAwaiter().GetResult().Trim()
        if ($_.Process.ExitCode -ne 0) { throw "Restricted expiry call failed: $err" }
        $out
    }) | Sort-Object
    if (($results -join ',') -ne 'f,t') { throw "Concurrent runners did not elect exactly one materializer: $($results -join ',')" }

    $repeat = Invoke-RunnerQuery -Sql $routineSql
    if ($repeat.ExitCode -ne 0 -or $repeat.Output.Trim() -ne 'f') { throw "A replay changed already-materialized state: $($repeat.Error) $($repeat.Output)" }
    $assertSql = @'
DO $verify$
DECLARE due_status text; due_revision bigint; due_sale uuid; due_events bigint; due_releases bigint;
        future_status text; future_revision bigint; future_releases bigint; on_hand bigint;
BEGIN
 SELECT status,revision,sale_id INTO due_status,due_revision,due_sale FROM pickup_reservations WHERE id='b0000000-0000-0000-0000-000000000001';
 SELECT count(*) INTO due_events FROM pickup_reservation_events WHERE reservation_id='b0000000-0000-0000-0000-000000000001' AND event_type='expired' AND actor_kind='system' AND actor_membership_id IS NULL;
 SELECT count(*) INTO due_releases FROM inventory_allocations WHERE pickup_reservation_id='b0000000-0000-0000-0000-000000000001' AND released_at IS NOT NULL AND release_reason='expired';
 SELECT status,revision INTO future_status,future_revision FROM pickup_reservations WHERE id='c0000000-0000-0000-0000-000000000001';
 SELECT count(*) INTO future_releases FROM inventory_allocations WHERE pickup_reservation_id='c0000000-0000-0000-0000-000000000001' AND released_at IS NOT NULL;
 SELECT stock_quantity INTO on_hand FROM branch_catalog_items WHERE product_id='a0000000-0000-0000-0000-000000000001' AND branch_id='60000000-0000-0000-0000-000000000006';
 IF due_status<>'expired' OR due_revision<>2 OR due_sale IS NOT NULL OR due_events<>1 OR due_releases<>1 THEN
   RAISE EXCEPTION 'Due hold did not atomically expire, append one system event and release one allocation.';
 END IF;
 IF future_status<>'held' OR future_revision<>1 OR future_releases<>0 THEN
   RAISE EXCEPTION 'Future hold changed before its deadline.';
 END IF;
 IF on_hand<>10 THEN RAISE EXCEPTION 'Expiry changed physical stock.'; END IF;
END
$verify$;
'@
    $assertFile = Join-Path $env:TEMP "pickup-expiry-assert-$PID.sql"
    $assertSql | Set-Content -LiteralPath $assertFile -Encoding utf8
    $assertCopy = Invoke-DockerCommand -Arguments @('cp', $assertFile, "${containerName}:/tmp/assert.sql")
    if ($assertCopy.ExitCode -ne 0) { throw "Could not copy expiry assertion SQL: $($assertCopy.Error)" }
    $assert = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'pos_migrator', '-d', 'advanced_pos', '-v', 'ON_ERROR_STOP=1', '-f', '/tmp/assert.sql')
    if ($assert.ExitCode -ne 0) { throw "Pickup expiry assertion failed: $($assert.Error) $($assert.Output)" }

    $noDue = Invoke-RunnerQuery -Sql $routineSql
    if ($noDue.ExitCode -ne 0 -or $noDue.Output.Trim() -ne 'f') { throw 'The runner did not become idle after all due holds were materialized.' }
    "All $($migrationFiles.Count) PostgreSQL migrations applied; concurrent execute-only runners materialized exactly one due hold, left a future hold unchanged, recorded one system event, released the matching allocation, preserved physical stock, and replayed idempotently."
}
finally {
    Remove-Item -LiteralPath (Join-Path $env:TEMP "pickup-expiry-seed-$PID.sql") -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $env:TEMP "pickup-expiry-assert-$PID.sql") -Force -ErrorAction SilentlyContinue
    $state = Invoke-DockerCommand -Arguments @('container','inspect',$containerName)
    if ($preserveContainer) {
        if ($state.ExitCode -eq 0) { Write-Warning "Preserving disposable PostgreSQL container for diagnosis: $containerName" }
    } elseif ($state.ExitCode -eq 0) {
        [void](Invoke-DockerCommand -Arguments @('rm','-f','-v',$containerName))
    }
}
