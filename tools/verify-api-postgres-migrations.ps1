$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$containerName = "advanced-pos-pg-check-$PID"
$preserveContainer = $env:POS_API_VERIFY_KEEP_CONTAINER -eq '1'
$temporarySql = [System.IO.Path]::GetTempFileName()
$hadMigrationUrl = Test-Path Env:POS_API_MIGRATION_DATABASE_URL
$previousMigrationUrl = $env:POS_API_MIGRATION_DATABASE_URL
$hadOperatorProvisionUrl = Test-Path Env:POS_OPERATOR_PROVISIONING_DATABASE_URL
$previousOperatorProvisionUrl = $env:POS_OPERATOR_PROVISIONING_DATABASE_URL
$hadOperatorProvisionActor = Test-Path Env:POS_OPERATOR_PROVISIONING_ACTOR
$previousOperatorProvisionActor = $env:POS_OPERATOR_PROVISIONING_ACTOR

function ConvertTo-WindowsProcessArgument {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes++
            continue
        }
        if ($character -eq '"') {
            if ($backslashes -gt 0) { [void]$builder.Append(('\' * (2 * $backslashes))) }
            [void]$builder.Append('\"')
        }
        else {
            if ($backslashes -gt 0) { [void]$builder.Append(('\' * $backslashes)) }
            [void]$builder.Append($character)
        }
        $backslashes = 0
    }
    if ($backslashes -gt 0) { [void]$builder.Append(('\' * (2 * $backslashes))) }
    [void]$builder.Append('"')
    $builder.ToString()
}

function Invoke-DockerCommand {
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [int]$TimeoutSeconds = 20
    )

    try {
        $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = (Get-Command docker -ErrorAction Stop).Source
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true
        # Windows PowerShell 5.1's .NET Framework ProcessStartInfo lacks ArgumentList.
        $argumentListProperty = $startInfo.GetType().GetProperty('ArgumentList')
        if ($null -ne $argumentListProperty) {
            $argumentList = $argumentListProperty.GetValue($startInfo)
            foreach ($argument in $Arguments) {
                [void]$argumentList.Add($argument)
            }
        }
        else {
            $startInfo.Arguments = (($Arguments | ForEach-Object { ConvertTo-WindowsProcessArgument -Value $_ }) -join ' ')
        }

        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $startInfo
        [void]$process.Start()
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit()
            [void]$stdoutTask.GetAwaiter().GetResult()
            [void]$stderrTask.GetAwaiter().GetResult()
            return [pscustomobject]@{ ExitCode = 124; Output = ''; Error = "Docker command timed out after $TimeoutSeconds seconds." }
        }

        [void]$process.WaitForExit()
        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            Output = $stdoutTask.GetAwaiter().GetResult()
            Error = $stderrTask.GetAwaiter().GetResult()
        }
    }
    catch {
        $failure = $_
        $detail = if ($null -ne $failure -and $null -ne $failure.Exception) {
            $failure.Exception.Message
        } else {
            [string]$failure
        }
        throw "Docker command '$($Arguments -join ' ')' failed: $detail"
    }
}

try {
    $password = "smoke-$([guid]::NewGuid().ToString('N'))"
    $runtimePassword = "runtime-$([guid]::NewGuid().ToString('N'))"
    $expiryRunnerPassword = "expiry-$([guid]::NewGuid().ToString('N'))"
    $attendanceRunnerPassword = "attendance-$([guid]::NewGuid().ToString('N'))"
    $started = Invoke-DockerCommand -Arguments @('run', '--rm', '-d', '--name', $containerName, '-e', "POSTGRES_PASSWORD=$password", '-p', '127.0.0.1::5432', 'postgres:16-alpine') -TimeoutSeconds 60
    if ($started.ExitCode -ne 0) {
        $details = (@($started.Error, $started.Output) | Where-Object { $_.Trim() }) -join [Environment]::NewLine
        throw "Could not start disposable PostgreSQL (exit $($started.ExitCode)). Is Docker Desktop running? $details"
    }
    $publishedPort = Invoke-DockerCommand -Arguments @('port', $containerName, '5432/tcp')
    if ($publishedPort.ExitCode -ne 0 -or -not $publishedPort.Output.Trim()) {
        throw 'Could not determine the disposable PostgreSQL port; Docker CLI calls are bounded to prevent indefinite waits.'
    }
    $port = ($publishedPort.Output.Trim() -split ':')[-1]

    $ready = $false
    for ($attempt = 0; $attempt -lt 45; $attempt++) {
        $probe = Invoke-DockerCommand -Arguments @('exec', $containerName, 'pg_isready', '-U', 'postgres', '-d', 'postgres')
        if ($probe.ExitCode -eq 124) { throw 'Docker did not respond while checking PostgreSQL readiness.' }
        if ($probe.ExitCode -eq 0) {
            $ready = $true
            break
        }
        Start-Sleep -Seconds 1
    }
    if (-not $ready) { throw 'Disposable PostgreSQL did not become ready.' }

    $runtimeDbRoles = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "CREATE ROLE pos_api_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_operator_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_operator_provisioner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_api_expiry_worker NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_api_expiry_runner LOGIN PASSWORD '$expiryRunnerPassword' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_attendance_worker NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; CREATE ROLE pos_attendance_runner LOGIN PASSWORD '$attendanceRunnerPassword' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS; GRANT CONNECT ON DATABASE postgres TO pos_api_expiry_runner, pos_attendance_runner;")
    if ($runtimeDbRoles.ExitCode -ne 0) { throw "Could not provision restricted runtime roles before migrations: $($runtimeDbRoles.Error)" }

    $env:POS_API_MIGRATION_DATABASE_URL = "postgres://postgres:$password@127.0.0.1:$port/postgres?sslmode=disable"
    # The shared Cargo wrapper owns cleanup policy and keeps the migration
    # executable available for both migration passes and the HTTP test run.
    # Set POS_API_VERIFY_REUSE_EXISTING_BUILD=1 when only the test harness changed.
    $reuseExistingBuild = $env:POS_API_VERIFY_REUSE_EXISTING_BUILD -eq '1'
    Push-Location $repoRoot
    try {
        if ($reuseExistingBuild) {
            if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'target\debug\pos-api.exe'))) {
                throw 'POS_API_VERIFY_REUSE_EXISTING_BUILD=1 requires an existing target\debug\pos-api.exe.'
            }
        }
        else {
            & (Join-Path $repoRoot 'tools\cargo-build.ps1') -Command build -CargoArgs @('--locked', '--package', 'pos-api', '--bins')
            if ($LASTEXITCODE -ne 0) { throw 'Could not build the API verifier binaries.' }
        }
        & (Join-Path $repoRoot 'target\debug\pos-api.exe') migrate
        if ($LASTEXITCODE -ne 0) { throw 'First migration run failed.' }
        & (Join-Path $repoRoot 'target\debug\pos-api.exe') migrate
        if ($LASTEXITCODE -ne 0) { throw 'Repeat migration run failed.' }

        $expiryPrivilegeCheck = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "DO `$`$ BEGIN IF NOT has_function_privilege('pos_api_expiry_runner','pos_worker.materialize_expired_pickup_reservation()','EXECUTE') OR has_function_privilege('pos_api_expiry_runner','pos_worker.materialize_next_expired_pickup_reservation()','EXECUTE') OR has_table_privilege('pos_api_expiry_runner','pickup_reservations','SELECT') OR has_table_privilege('pos_api_expiry_runner','pickup_reservation_events','INSERT') OR (SELECT rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit FROM pg_roles WHERE rolname='pos_api_expiry_runner') THEN RAISE EXCEPTION 'Pickup expiry runner privilege boundary is incorrect.'; END IF; END `$`$;")
        if ($expiryPrivilegeCheck.ExitCode -ne 0) { throw "Pickup expiry runner privilege check failed: $($expiryPrivilegeCheck.Error)" }

        $expiryRoutineCheck = Invoke-DockerCommand -Arguments @('exec', '-e', "PGPASSWORD=$expiryRunnerPassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_api_expiry_runner', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-At', '-c', 'SELECT pos_worker.materialize_expired_pickup_reservation();')
        if ($expiryRoutineCheck.ExitCode -ne 0 -or $expiryRoutineCheck.Output.Trim() -ne 'f') { throw "Execute-only pickup expiry runner could not invoke its routine: $($expiryRoutineCheck.Error) $($expiryRoutineCheck.Output)" }

        $expiryDirectRead = Invoke-DockerCommand -Arguments @('exec', '-e', "PGPASSWORD=$expiryRunnerPassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_api_expiry_runner', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', 'SELECT id FROM pickup_reservations LIMIT 1;')
        if ($expiryDirectRead.ExitCode -eq 0) { throw 'Pickup expiry runner unexpectedly has direct reservation-table access.' }

        $attendancePrivilegeCheck = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "DO `$`$ BEGIN IF NOT has_function_privilege('pos_attendance_runner','pos_worker.end_next_due_attendance_period()','EXECUTE') OR has_table_privilege('pos_attendance_runner','attendance_periods','SELECT') OR has_table_privilege('pos_attendance_runner','attendance_periods','UPDATE') OR has_table_privilege('pos_attendance_runner','attendance_events','INSERT') OR (SELECT rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit OR rolcanlogin FROM pg_roles WHERE rolname='pos_attendance_worker') OR (SELECT rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit FROM pg_roles WHERE rolname='pos_attendance_runner') THEN RAISE EXCEPTION 'Attendance worker role privilege boundary is incorrect.'; END IF; END `$`$;")
        if ($attendancePrivilegeCheck.ExitCode -ne 0) { throw "Attendance worker privilege check failed: $($attendancePrivilegeCheck.Error)" }

        $attendanceRoutineCheck = Invoke-DockerCommand -Arguments @('exec', '-e', "PGPASSWORD=$attendanceRunnerPassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_attendance_runner', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-At', '-c', 'SELECT pos_worker.end_next_due_attendance_period();')
        if ($attendanceRoutineCheck.ExitCode -ne 0 -or $attendanceRoutineCheck.Output.Trim() -ne 'f') { throw "Execute-only attendance runner could not invoke its routine: $($attendanceRoutineCheck.Error) $($attendanceRoutineCheck.Output)" }

        $attendanceDirectRead = Invoke-DockerCommand -Arguments @('exec', '-e', "PGPASSWORD=$attendanceRunnerPassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_attendance_runner', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', 'SELECT id FROM attendance_periods LIMIT 1;')
        if ($attendanceDirectRead.ExitCode -eq 0) { throw 'Attendance runner unexpectedly has direct attendance-table access.' }
    }
    finally {
        Pop-Location
    }

    $migrationDirectory = Join-Path $repoRoot 'apps\api\migrations\postgres'
    $expectedMigrationCount = @(
        Get-ChildItem -LiteralPath $migrationDirectory -Filter '*.sql' -File |
            Where-Object { $_.Name -match '^\d{4}_.+\.sql$' }
    ).Count
    $migrationCount = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-At', '-c', 'SELECT count(*) FROM _sqlx_migrations WHERE success;')
    if ($migrationCount.ExitCode -ne 0 -or $migrationCount.Output.Trim() -ne "$expectedMigrationCount") {
        throw "Expected $expectedMigrationCount successful migration records; found '$($migrationCount.Output.Trim())'."
    }

    @'
INSERT INTO tenants (id, display_name) VALUES
('10000000-0000-0000-0000-000000000001', 'Tenant A'),
('20000000-0000-0000-0000-000000000002', 'Tenant B');
INSERT INTO tenant_entitlements (tenant_id, entitlement_key, enabled, numeric_limit)
VALUES ('10000000-0000-0000-0000-000000000001', 'stations.active', true, 1);
INSERT INTO person_identities (id, issuer, subject) VALUES
('30000000-0000-0000-0000-000000000003', 'https://issuer.example', 'subject-a');
UPDATE person_identities SET email = 'shared@example.test' WHERE id = '30000000-0000-0000-0000-000000000003';
INSERT INTO person_identities (id, issuer, subject, email) VALUES
('70000000-0000-0000-0000-000000000007', 'https://other-issuer.example', 'subject-a', 'shared@example.test');
INSERT INTO person_identities (id, issuer, subject, display_name) VALUES
('31000000-0000-0000-0000-000000000031', 'https://issuer.example', 'staff-admin', 'Tenant Admin'),
('32000000-0000-0000-0000-000000000032', 'https://issuer.example', 'staff-user', 'Staff User'),
('33000000-0000-0000-0000-000000000033', 'https://issuer.example', 'branch-admin', 'Branch Admin');
INSERT INTO memberships (id, tenant_id, identity_id) VALUES
('40000000-0000-0000-0000-000000000004', '10000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000003'),
('41000000-0000-0000-0000-000000000041', '10000000-0000-0000-0000-000000000001', '31000000-0000-0000-0000-000000000031'),
('42000000-0000-0000-0000-000000000042', '10000000-0000-0000-0000-000000000001', '32000000-0000-0000-0000-000000000032'),
('43000000-0000-0000-0000-000000000043', '10000000-0000-0000-0000-000000000001', '33000000-0000-0000-0000-000000000033');
INSERT INTO branches (id, tenant_id, code, name, timezone) VALUES
('50000000-0000-0000-0000-000000000005', '10000000-0000-0000-0000-000000000001', 'A', 'A Branch', 'Africa/Accra'),
('60000000-0000-0000-0000-000000000006', '20000000-0000-0000-0000-000000000002', 'B', 'B Branch', 'Africa/Accra');
INSERT INTO stations (id, tenant_id, branch_id, code, name) VALUES
('80000000-0000-0000-0000-000000000008', '10000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000005', 'A1', 'A Station'),
('90000000-0000-0000-0000-000000000009', '20000000-0000-0000-0000-000000000002', '60000000-0000-0000-0000-000000000006', 'B1', 'B Station');
INSERT INTO membership_role_grants (tenant_id, membership_id, role_key, branch_id) VALUES
('10000000-0000-0000-0000-000000000001', '40000000-0000-0000-0000-000000000004', 'cashier', '50000000-0000-0000-0000-000000000005'),
('10000000-0000-0000-0000-000000000001', '41000000-0000-0000-0000-000000000041', 'admin', NULL),
('10000000-0000-0000-0000-000000000001', '42000000-0000-0000-0000-000000000042', 'cashier', '50000000-0000-0000-0000-000000000005'),
('10000000-0000-0000-0000-000000000001', '43000000-0000-0000-0000-000000000043', 'admin', '50000000-0000-0000-0000-000000000005');
INSERT INTO operator_accounts (id, issuer, subject, role) VALUES
('a0000000-0000-0000-0000-000000000001', 'https://operator.example', 'operator-a', 'support');
INSERT INTO hosted_payment_integration_requests
    (id,tenant_id,branch_id,created_by_membership_id,country_code,provider_name,capability,business_impact,details,idempotency_key,request_hash)
VALUES
('b0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','40000000-0000-0000-0000-000000000004','GH','Paystack','terminal confirmation','workaround_available','Test request','integration-test',decode(repeat('00',32),'hex'));
BEGIN;
INSERT INTO hosted_customers
    (id,tenant_id,name,created_by_membership_id,creation_idempotency_key,creation_request_hash)
VALUES
    ('c1000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','Credit Test Customer','41000000-0000-0000-0000-000000000041','credit-customer-seed',decode(repeat('11',32),'hex'));
INSERT INTO hosted_customer_credit_terms
    (id,tenant_id,customer_id,currency_code,credit_limit_minor,due_days,revision,last_reason,updated_by_membership_id)
VALUES
    ('c2000000-0000-0000-0000-000000000002','10000000-0000-0000-0000-000000000001','c1000000-0000-0000-0000-000000000001','GHS',5000,30,1,'Initial verifier terms','41000000-0000-0000-0000-000000000041');
INSERT INTO hosted_customer_credit_term_events
    (id,tenant_id,customer_id,currency_code,revision,credit_limit_minor,due_days,reason,actor_membership_id,idempotency_key,request_hash)
VALUES
    ('c3000000-0000-0000-0000-000000000003','10000000-0000-0000-0000-000000000001','c1000000-0000-0000-0000-000000000001','GHS',1,5000,30,'Initial verifier terms','41000000-0000-0000-0000-000000000041','credit-terms-seed',decode(repeat('22',32),'hex'));
COMMIT;
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO hosted_sales
    (id,tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,receipt_number,
     cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,
     customer_id,on_account_minor,request_hash,idempotency_key,status,created_at)
VALUES
    ('9a000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     '80000000-0000-0000-0000-000000000008','40000000-0000-0000-0000-000000000004',
     '9b000000-0000-0000-0000-000000000001','9c000000-0000-0000-0000-000000000001','RCV-TEST-001',
     'Verifier Cashier','Verifier Station','GHS',1000,0,1000,'c1000000-0000-0000-0000-000000000001',
     1000,decode(repeat('44',32),'hex'),'receivable-invoice-seed','completed',TIMESTAMPTZ '2026-11-07 12:00:00+00');
INSERT INTO hosted_receivable_invoices
    (id,tenant_id,branch_id,sale_id,customer_id,currency_code,original_amount_minor,credit_term_revision,
     due_days_snapshot,credit_limit_minor_snapshot,issued_at,due_at,issued_by_membership_id)
VALUES
    ('9d000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     '9a000000-0000-0000-0000-000000000001','c1000000-0000-0000-0000-000000000001','GHS',1000,1,30,5000,
     TIMESTAMPTZ '2026-11-07 12:00:00+00',TIMESTAMPTZ '2026-12-07 12:00:00+00','40000000-0000-0000-0000-000000000004');
COMMIT;
BEGIN;
SELECT set_config('pos.tenant_id','10000000-0000-0000-0000-000000000001',true);
INSERT INTO hosted_receivable_collections
    (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,
     external_reference,recorded_by_membership_id,idempotency_key,request_hash,received_at)
VALUES
    ('9e000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     'c1000000-0000-0000-0000-000000000001','GHS',400,'cash','cash_recorded',NULL,
     '41000000-0000-0000-0000-000000000041','receivable-partial-collection',decode(repeat('55',32),'hex'),TIMESTAMPTZ '2026-11-07 12:05:00+00');
INSERT INTO hosted_receivable_allocations
    (id,tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
VALUES
    ('9f000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     '9e000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','9d000000-0000-0000-0000-000000000001',
     'c1000000-0000-0000-0000-000000000001','GHS',400);
INSERT INTO hosted_receivable_collections
    (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,
     external_reference,recorded_by_membership_id,idempotency_key,request_hash,received_at)
VALUES
    ('9e100000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     'c1000000-0000-0000-0000-000000000001','GHS',200,'bank_transfer','manual_unverified','verifier-unconfirmed-transfer',
     '41000000-0000-0000-0000-000000000041','receivable-unconfirmed-transfer',decode(repeat('88',32),'hex'),TIMESTAMPTZ '2026-11-07 12:06:00+00');
INSERT INTO hosted_receivable_allocations
    (id,tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
VALUES
    ('9f100000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
     '9e100000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','9d000000-0000-0000-0000-000000000001',
     'c1000000-0000-0000-0000-000000000001','GHS',200);
COMMIT;
BEGIN;
SELECT set_config('pos.tenant_id','10000000-0000-0000-0000-000000000001',true);
DO $receivable_overallocation_guard$
DECLARE rejected boolean := false;
BEGIN
    BEGIN
        INSERT INTO hosted_receivable_collections
            (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,
             external_reference,recorded_by_membership_id,idempotency_key,request_hash,received_at)
        VALUES
            ('a0000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
             'c1000000-0000-0000-0000-000000000001','GHS',401,'bank_transfer','manual_unverified','verifier-overallocation',
             '41000000-0000-0000-0000-000000000041','receivable-overallocation',decode(repeat('66',32),'hex'),TIMESTAMPTZ '2026-11-07 12:10:00+00');
        INSERT INTO hosted_receivable_allocations
            (id,tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
        VALUES
            ('a1000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
             'a0000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','9d000000-0000-0000-0000-000000000001',
             'c1000000-0000-0000-0000-000000000001','GHS',401);
    EXCEPTION WHEN check_violation THEN
        rejected := true;
    END;
    IF NOT rejected THEN
        RAISE EXCEPTION 'Receivable allocation exceeded the invoice open balance without rejection.';
    END IF;
END
$receivable_overallocation_guard$;
DO $receivable_collection_total_guard$
DECLARE rejected boolean := false;
BEGIN
    BEGIN
        INSERT INTO hosted_receivable_collections
            (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,
             external_reference,recorded_by_membership_id,idempotency_key,request_hash,received_at)
        VALUES
            ('a2000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
             'c1000000-0000-0000-0000-000000000001','GHS',200,'bank_transfer','manual_unverified','verifier-mismatched-total',
             '41000000-0000-0000-0000-000000000041','receivable-mismatched-total',decode(repeat('77',32),'hex'),TIMESTAMPTZ '2026-11-07 12:15:00+00');
        INSERT INTO hosted_receivable_allocations
            (id,tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
        VALUES
            ('a3000000-0000-0000-0000-000000000001','10000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005',
             'a2000000-0000-0000-0000-000000000001','50000000-0000-0000-0000-000000000005','9d000000-0000-0000-0000-000000000001',
             'c1000000-0000-0000-0000-000000000001','GHS',100);
        SET CONSTRAINTS hosted_receivable_collection_requires_full_allocation,
                        hosted_receivable_allocation_matches_collection IMMEDIATE;
    EXCEPTION WHEN check_violation THEN
        rejected := true;
    END;
    IF NOT rejected THEN
        RAISE EXCEPTION 'Mismatched receivable collection and allocation totals committed without rejection.';
    END IF;
END
$receivable_collection_total_guard$;
COMMIT;
DO $identity_check$
DECLARE matched_id text;
BEGIN
    SELECT id::text INTO matched_id FROM person_identities
    WHERE issuer = 'https://issuer.example' AND subject = 'subject-a';
    IF matched_id <> '30000000-0000-0000-0000-000000000003' THEN
        RAISE EXCEPTION 'Exact issuer/subject lookup returned the wrong identity.';
    END IF;
    SELECT id::text INTO matched_id FROM person_identities
    WHERE issuer = 'https://other-issuer.example' AND subject = 'subject-a';
    IF matched_id <> '70000000-0000-0000-0000-000000000007' THEN
        RAISE EXCEPTION 'Same subject under another issuer did not remain distinct.';
    END IF;
    IF (SELECT count(*) FROM person_identities WHERE email = 'shared@example.test') <> 2 THEN
        RAISE EXCEPTION 'The schema unexpectedly treats email as a unique identity key.';
    END IF;
END
$identity_check$;
ALTER ROLE pos_api_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
\i /tmp/grant-runtime.sql
SET ROLE pos_api_runtime;
BEGIN;
SELECT set_config('pos.verified_issuer', 'https://issuer.example', true),
       set_config('pos.verified_subject', 'subject-a', true);
DO $runtime_permissions$
DECLARE matched_id text;
BEGIN
    SELECT id::text INTO matched_id FROM person_identities
    WHERE issuer = 'https://issuer.example' AND subject = 'subject-a';
    IF matched_id <> '30000000-0000-0000-0000-000000000003' THEN
        RAISE EXCEPTION 'Restricted runtime identity lookup failed.';
    END IF;
    IF (SELECT count(*) FROM person_identities) <> 1 THEN
        RAISE EXCEPTION 'Verified identity context exposed another provider identity.';
    END IF;
    BEGIN
        PERFORM email FROM person_identities LIMIT 1;
        RAISE EXCEPTION 'Runtime role unexpectedly read identity email.';
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'Runtime role cannot read unneeded email.';
    END;
    IF (SELECT count(*) FROM pg_class c
        WHERE c.oid IN ('stock_transfers'::regclass, 'stock_transfer_lines'::regclass, 'stock_transfer_events'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 3 THEN
        RAISE EXCEPTION 'Every stock transfer table must force row-level security.';
    END IF;
    IF (SELECT count(*) FROM pg_policies
        WHERE schemaname = current_schema()
          AND tablename IN ('stock_transfers', 'stock_transfer_lines', 'stock_transfer_events')) <> 8 THEN
        RAISE EXCEPTION 'The stock transfer tables are missing expected branch-side RLS policies.';
    END IF;
    IF NOT has_column_privilege('pos_api_runtime', 'stock_transfers', 'status', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'stock_transfer_events', 'details', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'stock_transfer_lines', 'received_quantity', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'stock_transfer_events', 'DELETE') THEN
        RAISE EXCEPTION 'Restricted stock transfer runtime grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='attendance_schedules'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='attendance_periods'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='attendance_events'::regclass)
       OR NOT has_column_privilege('pos_api_runtime','attendance_schedules','weekdays','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','attendance_schedules','revision','UPDATE')
       OR NOT has_column_privilege('pos_api_runtime','attendance_periods','started_at','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','attendance_periods','ended_at','UPDATE')
       OR NOT has_column_privilege('pos_api_runtime','attendance_events','event_type','INSERT')
       OR has_column_privilege('pos_api_runtime','attendance_periods','scheduled_start_at','UPDATE')
       OR has_table_privilege('pos_api_runtime','attendance_events','UPDATE')
       OR has_table_privilege('pos_api_runtime','attendance_events','DELETE')
       OR has_table_privilege('pos_api_runtime','attendance_schedules','DELETE') THEN
        RAISE EXCEPTION 'Attendance forced RLS or least-column grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='pickup_reservations'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='pickup_reservation_lines'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='pickup_reservation_events'::regclass)
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservations', 'revision', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservations', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservation_lines', 'quantity', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservation_events', 'event_type', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservation_events', 'reason', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'pickup_reservation_events', 'event_type', 'INSERT')
       OR has_table_privilege('pos_api_runtime', 'pickup_reservations', 'DELETE')
       OR NOT has_table_privilege('pos_api_runtime', 'pickup_reservation_events', 'SELECT')
       OR has_table_privilege('pos_api_runtime', 'pickup_reservation_events', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'pickup_reservation_events', 'DELETE')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='pickup_reservations'::regclass AND tgname='pickup_reservations_transition_guard' AND NOT tgisinternal)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='pickup_reservations'::regclass AND tgname='pickup_reservation_header_event_check' AND NOT tgisinternal)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='pickup_reservation_events'::regclass AND tgname='pickup_reservation_event_state_check' AND NOT tgisinternal) THEN
        RAISE EXCEPTION 'Pickup reservation forced RLS or append-only runtime grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_customers'::regclass)
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_customers', 'phone', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_customers', 'revision', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_customers', 'creation_request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_customers', 'revision', 'UPDATE')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sales', 'customer_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sales', 'customer_id', 'SELECT')
       OR has_table_privilege('pos_api_runtime', 'hosted_customers', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted customer isolation or restricted column grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_sale_customer_links'::regclass)
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'customer_id', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'linked_by_membership_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'customer_name_snapshot', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'customer_name_snapshot', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sales', 'customer_name_snapshot', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sales', 'customer_name_snapshot', 'INSERT')
       OR has_table_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_sale_customer_links', 'DELETE')
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_sale_customer_links') <> 2 THEN
        RAISE EXCEPTION 'Hosted sale customer association RLS or append-only grants are missing or too broad.';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_customers' AND policyname='hosted_customers_branch_update' AND cmd='UPDATE') THEN
        RAISE EXCEPTION 'Hosted customer management update policy is missing.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_customer_merges'::regclass)
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_merges','request_hash','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_merges','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_customers','merged_into_customer_id','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_customer_merges','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_customer_merges','DELETE')
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_customer_merges') <> 2 THEN
        RAISE EXCEPTION 'Hosted customer merge RLS or append-only least-column grants are missing or too broad.';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_customers'::regclass AND tgname='hosted_customer_merge_event_required' AND tgdeferrable AND tginitdeferred AND NOT tgisinternal) THEN
        RAISE EXCEPTION 'Merged customer rows must be protected by the deferred matching-event constraint trigger.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_suppliers'::regclass)
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_suppliers', 'contact_person', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_suppliers', 'creation_request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_suppliers', 'revision', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_suppliers', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted supplier isolation or restricted column grants are missing or too broad.';
    END IF;
    IF (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_suppliers') <> 3 THEN
        RAISE EXCEPTION 'Hosted supplier read/insert/update policies are incomplete.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_purchase_orders'::regclass, 'hosted_purchase_order_lines'::regclass,
          'hosted_purchase_receipts'::regclass, 'hosted_purchase_receipt_lines'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 4
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN (
          'hosted_purchase_orders', 'hosted_purchase_order_lines', 'hosted_purchase_receipts', 'hosted_purchase_receipt_lines')) <> 12 THEN
        RAISE EXCEPTION 'Hosted purchase order and receipt tables require forced RLS and complete branch policies.';
    END IF;
    IF NOT has_column_privilege('pos_api_runtime', 'hosted_purchase_orders', 'creation_request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_purchase_orders', 'revision', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_purchase_order_lines', 'received_quantity', 'UPDATE')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_purchase_receipts', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'branch_catalog_items', 'cost_minor', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_purchase_orders', 'DELETE')
       OR has_table_privilege('pos_api_runtime', 'hosted_purchase_receipts', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted purchasing least-privilege runtime grants are missing or too broad.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_supplier_bills'::regclass, 'hosted_supplier_bill_allocations'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 2
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN (
          'hosted_supplier_bills', 'hosted_supplier_bill_allocations')) <> 4 THEN
        RAISE EXCEPTION 'Hosted supplier bills require forced RLS and complete branch policies.';
    END IF;
    IF NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_bills', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_bill_allocations', 'amount_minor', 'INSERT')
       OR has_table_privilege('pos_api_runtime', 'hosted_supplier_bills', 'DELETE')
       OR has_table_privilege('pos_api_runtime', 'hosted_supplier_bill_allocations', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted supplier bill least-privilege grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_payment_integration_requests'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_payment_integration_requests') <> 4 THEN
        RAISE EXCEPTION 'Hosted payment integration requests require forced RLS and complete tenant/operator policies.';
    END IF;
    IF NOT has_column_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'request_hash', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'business_impact', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'details', 'SELECT')
       OR has_column_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'status', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_payment_integration_requests', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted payment integration request runtime grants are missing or too broad.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_support_requests'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_support_requests') <> 4
       OR NOT has_column_privilege('pos_api_runtime','hosted_support_requests','request_hash','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_support_requests','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_support_requests','details','SELECT')
       OR has_table_privilege('pos_api_runtime','hosted_support_requests','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_support_requests','DELETE')
       OR NOT has_column_privilege('pos_operator_runtime','hosted_support_requests','customer_visible_update','UPDATE')
       OR has_column_privilege('pos_operator_runtime','hosted_support_requests','request_hash','SELECT') THEN
        RAISE EXCEPTION 'Hosted support request tenant/operator isolation or column grants are incorrect.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='operator_support_issue_support_requests'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='operator_support_request_events'::regclass)
       OR has_table_privilege('pos_api_runtime','operator_support_issue_support_requests','SELECT')
       OR has_table_privilege('pos_api_runtime','operator_support_request_events','SELECT')
       OR has_table_privilege('pos_operator_runtime','operator_support_issue_support_requests','DELETE')
       OR has_table_privilege('pos_operator_runtime','operator_support_request_events','UPDATE')
       OR has_table_privilege('pos_operator_runtime','operator_support_request_events','DELETE')
       OR NOT has_column_privilege('pos_operator_runtime','operator_support_issue_events','linked_support_request_id','INSERT') THEN
        RAISE EXCEPTION 'Operator support request link/history RLS or append-only grants are incorrect.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_entity_images'::regclass)
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_entity_images', 'image_data', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_entity_images', 'updated_by_membership_id', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_entity_images', 'image_data', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_entity_images', 'updated_at', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_entity_images', 'UPDATE')
       OR NOT EXISTS (
          SELECT 1 FROM pg_policies
          WHERE schemaname=current_schema()
            AND tablename='hosted_entity_images'
            AND policyname='hosted_staff_images_owner_admin_only'
            AND permissive='RESTRICTIVE'
       )
       OR NOT EXISTS (
          SELECT 1 FROM pg_constraint
          WHERE conrelid='hosted_entity_images'::regclass
            AND conname='hosted_entity_images_entity_type_check'
            AND pg_get_constraintdef(oid) LIKE '%staff%'
       ) THEN
        RAISE EXCEPTION 'Hosted entity image RLS or least-privilege grants are missing or too broad.';
    END IF;
    IF NOT (SELECT NOT rolsuper AND NOT rolbypassrls AND NOT rolinherit
            FROM pg_roles WHERE rolname='pos_operator_runtime')
       OR NOT has_column_privilege('pos_operator_runtime', 'hosted_payment_integration_requests', 'status', 'UPDATE')
       OR has_column_privilege('pos_operator_runtime', 'hosted_payment_integration_requests', 'details', 'UPDATE')
       OR has_table_privilege('pos_operator_runtime', 'operator_payment_request_events', 'DELETE')
       OR has_table_privilege('pos_operator_runtime', 'operator_audit_events', 'SELECT')
       OR has_table_privilege('pos_operator_runtime', 'operator_accounts', 'INSERT')
       OR has_table_privilege('pos_operator_runtime', 'memberships', 'SELECT')
       OR has_table_privilege('pos_operator_runtime', 'membership_role_grants', 'SELECT') THEN
        RAISE EXCEPTION 'Operator runtime role is missing isolation or has excessive privileges.';
    END IF;
    IF NOT (SELECT NOT rolsuper AND NOT rolbypassrls AND NOT rolinherit
            FROM pg_roles WHERE rolname='pos_operator_provisioner')
       OR NOT has_column_privilege('pos_operator_provisioner', 'operator_accounts', 'status', 'UPDATE')
       OR NOT has_column_privilege('pos_operator_provisioner', 'operator_provisioning_events', 'reason', 'INSERT')
       OR has_table_privilege('pos_operator_provisioner', 'operator_provisioning_events', 'UPDATE')
       OR has_table_privilege('pos_operator_provisioner', 'operator_accounts', 'DELETE')
       OR has_table_privilege('pos_operator_provisioner', 'memberships', 'SELECT')
       OR has_table_privilege('pos_operator_runtime', 'operator_accounts', 'UPDATE') THEN
        RAISE EXCEPTION 'Operator provisioning role is missing least-privilege account lifecycle grants.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_supplier_payments'::regclass, 'hosted_supplier_payment_allocations'::regclass,
          'hosted_accounting_journals'::regclass, 'hosted_accounting_journal_lines'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 4
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN (
          'hosted_supplier_payments', 'hosted_supplier_payment_allocations',
          'hosted_accounting_journals', 'hosted_accounting_journal_lines')) <> 25
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_station_cash_expense_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_cashier_sale_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_sale_refund_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journal_lines' AND policyname='hosted_accounting_journal_lines_sale_refund_insert' AND cmd='INSERT') THEN
        RAISE EXCEPTION 'Hosted supplier payments, expenses, and accounting journals require forced RLS and complete branch policies.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_supplier_credit_notes'::regclass, 'hosted_supplier_credit_lines'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 2
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN (
          'hosted_supplier_credit_notes','hosted_supplier_credit_lines')) <> 4
       OR NOT has_column_privilege('pos_api_runtime','hosted_supplier_credit_notes','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_supplier_credit_notes','total_minor','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_supplier_credit_lines','quantity','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_supplier_credit_lines','amount_minor','SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_supplier_credit_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journal_lines' AND policyname='hosted_accounting_journal_lines_supplier_credit_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_supplier_credit_notes'::regclass AND tgname='hosted_supplier_credit_note_total_matches_lines' AND NOT tgisinternal)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_supplier_credit_lines'::regclass AND tgname='hosted_supplier_credit_line_total_matches_note' AND NOT tgisinternal)
       OR has_table_privilege('pos_api_runtime','hosted_supplier_credit_notes','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_supplier_credit_notes','DELETE')
       OR has_table_privilege('pos_api_runtime','hosted_supplier_credit_lines','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_supplier_credit_lines','DELETE') THEN
        RAISE EXCEPTION 'Hosted supplier credits require forced RLS, source journal policies and append-only least-column grants.';
    END IF;
    IF NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_payments', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_payments', 'cash_location_id', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_payments', 'cash_location_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_payment_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_payment_id', 'SELECT')
       OR has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_payment_id', 'UPDATE')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_supplier_payment_allocations', 'amount_minor', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_accounting_journals', 'source_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_accounting_journals', 'coverage_status', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'branch_catalog_items', 'cost_minor', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_lines', 'unit_cost_minor_snapshot', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_lines', 'unit_cost_minor_snapshot', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_accounting_journal_lines', 'debit_minor', 'INSERT')
       OR has_table_privilege('pos_api_runtime', 'hosted_supplier_payments', 'DELETE')
       OR has_table_privilege('pos_api_runtime', 'hosted_supplier_payment_allocations', 'DELETE')
       OR has_table_privilege('pos_api_runtime', 'hosted_accounting_journals', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_accounting_journal_lines', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted supplier settlement or journal least-privilege grants are missing or too broad.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_sale_refunds'::regclass, 'hosted_sale_refund_lines'::regclass,
          'hosted_sale_refund_tenders'::regclass) AND c.relrowsecurity AND c.relforcerowsecurity) <> 3
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN (
          'hosted_sale_refunds', 'hosted_sale_refund_lines', 'hosted_sale_refund_tenders')) <> 6
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_refunds', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_refund_lines', 'restock', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_sale_refund_tenders', 'external_reference', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_refund_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_refund_id', 'SELECT')
       OR has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_refund_id', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_sale_refunds', 'DELETE')
       OR has_table_privilege('pos_api_runtime', 'hosted_sale_refund_lines', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_sale_refund_tenders', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted refunds require forced RLS and append-only least-column runtime grants.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_expenses'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_expenses') <> 4
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'request_hash', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'amount_minor', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'drawer_shift_id', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'drawer_shift_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'cash_location_id', 'SELECT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_expenses', 'cash_location_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_expense_id', 'INSERT')
       OR NOT has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_expense_id', 'SELECT')
       OR has_column_privilege('pos_api_runtime', 'hosted_drawer_movements', 'source_expense_id', 'UPDATE')
       OR has_column_privilege('pos_api_runtime', 'hosted_expenses', 'amount_minor', 'UPDATE')
       OR has_table_privilege('pos_api_runtime', 'hosted_expenses', 'DELETE') THEN
        RAISE EXCEPTION 'Hosted expenses require forced RLS and append-only least-column runtime grants.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_expense_reimbursements'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_expense_reimbursements') <> 2
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_reimbursements','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_reimbursements','amount_minor','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_reimbursements','amount_minor','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_accounting_journals','source_type','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_accounting_journals','source_id','INSERT')
       OR has_column_privilege('pos_api_runtime','hosted_expense_reimbursements','amount_minor','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_expense_reimbursements','DELETE') THEN
        RAISE EXCEPTION 'Hosted expense reimbursements require forced RLS and append-only least-column runtime grants.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_expense_cost_center_reclassifications'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_expense_cost_center_reclassifications') <> 2
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','next_kind','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','reason','INSERT')
       OR has_column_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','reason','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_expense_cost_center_reclassifications','DELETE') THEN
        RAISE EXCEPTION 'Hosted expense cost center corrections require forced RLS and append-only least-column runtime grants.';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_sales' AND policyname='hosted_sales_customer_visible' AND permissive='RESTRICTIVE') THEN
        RAISE EXCEPTION 'Hosted sale customer linking does not enforce customer visibility.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_cash_locations'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_cash_transfers'::regclass)
       OR NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_cash_location_events'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename IN ('hosted_cash_locations','hosted_cash_transfers','hosted_cash_location_events')) <> 20
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_locations' AND policyname='hosted_cash_locations_cashier_register_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_locations' AND policyname='hosted_cash_locations_cashier_drop_destination_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_safe_opening_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_drop_inflow_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_drop_inflow_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_drawer_movement_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_drawer_movement_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_source_outflow_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_source_outflow_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_collection_inflow_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_location_events' AND policyname='hosted_cash_events_cashier_collection_inflow_read' AND cmd='SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_locations','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_locations','id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_transfers','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_transfers','id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','signed_amount_minor','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_expense_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_expense_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_supplier_payment_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_supplier_payment_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_expense_reimbursement_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_expense_reimbursement_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_sale_tender_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_sale_tender_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_refund_tender_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_refund_tender_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_drawer_movement_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_drawer_movement_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_drawer_movements','cash_destination_location_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_drawer_movements','cash_destination_location_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_drawer_movements','source_receivable_collection_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_drawer_movements','source_receivable_collection_id','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_receivable_collection_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_location_events','source_receivable_collection_id','SELECT')
       OR has_column_privilege('pos_api_runtime','hosted_cash_locations','status','UPDATE')
       OR has_column_privilege('pos_api_runtime','hosted_cash_transfers','amount_minor','UPDATE')
       OR has_column_privilege('pos_api_runtime','hosted_cash_location_events','signed_amount_minor','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_cash_location_events','DELETE') THEN
        RAISE EXCEPTION 'Hosted cash locations require forced RLS and append-only least-column grants.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_cash_reconciliations'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_cash_reconciliations') <> 2
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_reconciliations','request_hash','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_reconciliations','request_hash','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_reconciliations','id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_cash_reconciliations','adjustment_minor','INSERT')
       OR has_column_privilege('pos_api_runtime','hosted_cash_reconciliations','reason','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_cash_reconciliations','DELETE')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_cash_reconciliation_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journal_lines' AND policyname='hosted_accounting_journal_lines_cash_reconciliation_insert' AND cmd='INSERT') THEN
        RAISE EXCEPTION 'Cash reconciliation requires forced RLS, owner/admin journal policies, and append-only least-column grants.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
          'hosted_customer_credit_terms'::regclass,'hosted_customer_credit_term_events'::regclass)
          AND c.relrowsecurity AND c.relforcerowsecurity) <> 2
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema()
           AND tablename IN ('hosted_customer_credit_terms','hosted_customer_credit_term_events')) <> 6
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_credit_terms','credit_limit_minor','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_credit_terms','credit_limit_minor','UPDATE')
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_credit_terms','revision','UPDATE')
       OR has_column_privilege('pos_api_runtime','hosted_customer_credit_terms','customer_id','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_customer_credit_terms','DELETE')
       OR NOT has_column_privilege('pos_api_runtime','hosted_customer_credit_term_events','request_hash','INSERT')
       OR has_table_privilege('pos_api_runtime','hosted_customer_credit_term_events','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_customer_credit_term_events','DELETE')
       OR NOT has_function_privilege('pos_api_runtime','pos_worker.customer_credit_position(uuid)','EXECUTE')
       OR EXISTS (SELECT 1 FROM pg_proc p CROSS JOIN LATERAL
           aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) privilege
           JOIN pg_namespace n ON n.oid=p.pronamespace
           WHERE n.nspname='pos_worker' AND p.proname='customer_credit_position'
             AND privilege.grantee=0 AND privilege.privilege_type='EXECUTE')
       OR pg_has_role('pos_api_runtime','pos_receivables_worker','MEMBER')
       OR NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='pos_receivables_worker'
           AND NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
           AND NOT rolinherit AND NOT rolbypassrls)
       OR NOT EXISTS (SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
           JOIN pg_roles r ON r.oid=p.proowner
           WHERE n.nspname='pos_worker' AND p.proname='customer_credit_position'
             AND r.rolname='pos_receivables_worker')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_customer_credit_terms'::regclass
           AND tgname='hosted_customer_credit_term_event_required' AND tgdeferrable AND tginitdeferred AND NOT tgisinternal)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_customer_credit_term_events'::regclass
           AND tgname='hosted_customer_credit_term_event_state_matches' AND tgdeferrable AND tginitdeferred AND NOT tgisinternal) THEN
        RAISE EXCEPTION 'Customer credit terms require forced RLS, matched deferred history, and least-column grants.';
    END IF;
    IF NOT (SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid='hosted_receivable_invoices'::regclass)
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_receivable_invoices') <> 3
       OR NOT has_column_privilege('pos_api_runtime','hosted_sales','on_account_minor','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_sales','on_account_minor','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_receivable_invoices','original_amount_minor','SELECT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_receivable_invoices','original_amount_minor','INSERT')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_invoices','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_invoices','DELETE')
       OR (SELECT count(*) FROM pg_trigger WHERE tgrelid IN (
           'hosted_sales'::regclass,'hosted_sale_tenders'::regclass,'hosted_receivable_invoices'::regclass)
           AND tgname IN ('hosted_receivable_sale_state_from_sale','hosted_receivable_sale_state_from_tender','hosted_receivable_sale_state_from_invoice')
           AND tgdeferrable AND tginitdeferred AND NOT tgisinternal) <> 3 THEN
        RAISE EXCEPTION 'Receivable invoices require forced RLS, immutable least-column access and deferred sale-balance guards.';
    END IF;
    IF (SELECT count(*) FROM pg_class c WHERE c.oid IN (
           'hosted_receivable_collections'::regclass,'hosted_receivable_allocations'::regclass)
           AND c.relrowsecurity AND c.relforcerowsecurity) <> 2
       OR (SELECT count(*) FROM pg_policies WHERE schemaname=current_schema()
           AND tablename IN ('hosted_receivable_collections','hosted_receivable_allocations')) <> 6
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_invoices','branch_id','SELECT')
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_invoices','original_amount_minor','SELECT')
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_allocations','amount_minor','SELECT')
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_collections','amount_minor','SELECT')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_collections','SELECT')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_collections','INSERT')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_allocations','SELECT')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_allocations','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_receivable_collections','drawer_shift_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_receivable_collections','station_id','INSERT')
       OR NOT has_column_privilege('pos_api_runtime','hosted_receivable_allocations','amount_minor','INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_receivable_collection_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journals' AND policyname='hosted_accounting_journals_receivable_collection_station_read' AND cmd='SELECT')
       OR NOT EXISTS (SELECT 1 FROM pg_policies WHERE schemaname=current_schema() AND tablename='hosted_accounting_journal_lines' AND policyname='hosted_accounting_journal_lines_receivable_collection_insert' AND cmd='INSERT')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='hosted_receivable_allocations'::regclass
           AND tgname='hosted_receivable_allocations_open_balance_guard' AND NOT tgisinternal)
       OR (SELECT count(*) FROM pg_trigger WHERE tgrelid IN (
           'hosted_receivable_collections'::regclass,'hosted_receivable_allocations'::regclass)
           AND tgname IN ('hosted_receivable_collection_requires_full_allocation',
                          'hosted_receivable_allocation_matches_collection')
           AND tgdeferrable AND tginitdeferred AND NOT tgisinternal) <> 2
       OR has_table_privilege('pos_api_runtime','hosted_receivable_collections','UPDATE')
       OR has_table_privilege('pos_api_runtime','hosted_receivable_allocations','DELETE') THEN
        RAISE EXCEPTION 'Receivable collections and allocations require forced RLS, worker-only aggregation, immutable writes and allocation guards.';
    END IF;
    IF NOT has_function_privilege('pos_api_runtime','pos_worker.customer_open_invoices(uuid)','EXECUTE')
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_invoices','due_at','SELECT')
       OR NOT has_column_privilege('pos_receivables_worker','hosted_receivable_allocations','invoice_id','SELECT')
       OR EXISTS (
           SELECT 1 FROM pg_proc p
           JOIN pg_namespace n ON n.oid=p.pronamespace
           CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) a
           WHERE n.nspname='pos_worker' AND p.proname='customer_open_invoices'
             AND a.grantee=0 AND a.privilege_type='EXECUTE'
       ) THEN
        RAISE EXCEPTION 'Open receivable summaries must be available only through the authenticated cashier capability.';
    END IF;
END
$runtime_permissions$;
SELECT set_config('pos.identity_id', '31000000-0000-0000-0000-000000000031', true),
       set_config('pos.tenant_id', '10000000-0000-0000-0000-000000000001', true),
       set_config('pos.membership_id', '41000000-0000-0000-0000-000000000041', true),
       set_config('pos.branch_id', '50000000-0000-0000-0000-000000000005', true),
       set_config('pos.station_id', '', true),
       set_config('pos.staff_roster_tenant_id', '10000000-0000-0000-0000-000000000001', true);
DO $credit_term_rls$
DECLARE visible_count integer;
DECLARE changed_count integer;
BEGIN
    PERFORM set_config('pos.identity_id','30000000-0000-0000-0000-000000000003',true),
           set_config('pos.membership_id','40000000-0000-0000-0000-000000000004',true),
           set_config('pos.station_id','80000000-0000-0000-0000-000000000008',true);
    SELECT count(*) INTO visible_count FROM hosted_customer_credit_terms
      WHERE customer_id='c1000000-0000-0000-0000-000000000001';
    IF visible_count <> 1 THEN
        RAISE EXCEPTION 'Authorized cashier branch should read the customer currency credit terms.';
    END IF;
    UPDATE hosted_customer_credit_terms SET credit_limit_minor=9000,revision=2,
        last_reason='cashier must not change terms',updated_by_membership_id='40000000-0000-0000-0000-000000000004'
      WHERE customer_id='c1000000-0000-0000-0000-000000000001' AND revision=1;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    IF changed_count <> 0 THEN
        RAISE EXCEPTION 'Cashier unexpectedly changed tenant-wide credit terms.';
    END IF;

    PERFORM set_config('pos.identity_id','33000000-0000-0000-0000-000000000033',true);
    PERFORM set_config('pos.membership_id','43000000-0000-0000-0000-000000000043',true);
    PERFORM set_config('pos.station_id','',true);
    UPDATE hosted_customer_credit_terms SET credit_limit_minor=9000,revision=2,
        last_reason='branch admin must not change tenant-wide terms',updated_by_membership_id='43000000-0000-0000-0000-000000000043'
      WHERE customer_id='c1000000-0000-0000-0000-000000000001' AND revision=1;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    IF changed_count <> 0 THEN
        RAISE EXCEPTION 'Branch-scoped administrator unexpectedly changed tenant-wide credit terms.';
    END IF;

    PERFORM set_config('pos.identity_id','31000000-0000-0000-0000-000000000031',true);
    PERFORM set_config('pos.membership_id','41000000-0000-0000-0000-000000000041',true);
    PERFORM set_config('pos.station_id','',true);
    UPDATE hosted_customer_credit_terms SET credit_limit_minor=6000,revision=2,
        last_reason='Owner approved updated credit limit',updated_by_membership_id='41000000-0000-0000-0000-000000000041',updated_at=now()
      WHERE customer_id='c1000000-0000-0000-0000-000000000001' AND revision=1;
    INSERT INTO hosted_customer_credit_term_events
        (tenant_id,customer_id,currency_code,revision,previous_credit_limit_minor,previous_due_days,
         credit_limit_minor,due_days,reason,actor_membership_id,idempotency_key,request_hash)
    VALUES
        ('10000000-0000-0000-0000-000000000001','c1000000-0000-0000-0000-000000000001','GHS',2,5000,30,
         6000,30,'Owner approved updated credit limit','41000000-0000-0000-0000-000000000041','credit-terms-update',decode(repeat('33',32),'hex'));
    SELECT count(*) INTO visible_count FROM hosted_customer_credit_term_events
      WHERE customer_id='c1000000-0000-0000-0000-000000000001';
    IF visible_count <> 2 THEN
        RAISE EXCEPTION 'Tenant administrator should see the append-only customer credit revision history.';
    END IF;
    BEGIN
        UPDATE hosted_customer_credit_term_events SET reason='rewrite history'
          WHERE customer_id='c1000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'Runtime unexpectedly edited immutable customer credit history.';
    EXCEPTION WHEN insufficient_privilege THEN
        NULL;
    END;
END
$credit_term_rls$;
SET CONSTRAINTS hosted_customer_credit_term_event_required, hosted_customer_credit_term_event_state_matches IMMEDIATE;
DO $credit_position_capability$
DECLARE position record;
BEGIN
    PERFORM set_config('pos.inventory_availability','enabled',true);
    SELECT * INTO position FROM pos_worker.customer_credit_position(
        'c1000000-0000-0000-0000-000000000001'
    );
    IF position.currency_code <> 'GHS' OR NOT position.has_credit_terms
       OR position.terms_revision <> 2 OR position.credit_limit_minor <> '6000'
       OR position.due_days <> 30 OR position.outstanding_minor <> '600'
       OR position.available_minor <> '5400'
       OR current_setting('pos.inventory_availability',true)='enabled' THEN
        RAISE EXCEPTION 'Restricted customer credit position returned incorrect terms or aggregate.';
    END IF;
END
$credit_position_capability$;
DO $cashier_open_receivable_capability$
DECLARE invoice_count integer;
DECLARE open_amount text;
BEGIN
    PERFORM set_config('pos.identity_id','30000000-0000-0000-0000-000000000003',true),
           set_config('pos.tenant_id','10000000-0000-0000-0000-000000000001',true),
           set_config('pos.membership_id','40000000-0000-0000-0000-000000000004',true),
           set_config('pos.branch_id','50000000-0000-0000-0000-000000000005',true),
           set_config('pos.station_id','80000000-0000-0000-0000-000000000008',true);
    SELECT count(*),min(open_minor)::text INTO invoice_count,open_amount
      FROM pos_worker.customer_open_invoices('c1000000-0000-0000-0000-000000000001');
    IF invoice_count <> 1 OR open_amount <> '400'
       OR current_setting('pos.inventory_availability',true)='enabled' THEN
        RAISE EXCEPTION 'Cashier open-invoice capability returned an incorrect balance or retained inventory scope.';
    END IF;
END
$cashier_open_receivable_capability$;
SELECT set_config('pos.identity_id', '31000000-0000-0000-0000-000000000031', true),
       set_config('pos.tenant_id', '10000000-0000-0000-0000-000000000001', true),
       set_config('pos.membership_id', '41000000-0000-0000-0000-000000000041', true),
       set_config('pos.branch_id', '50000000-0000-0000-0000-000000000005', true),
       set_config('pos.station_id', '', true),
       set_config('pos.staff_roster_tenant_id', '10000000-0000-0000-0000-000000000001', true);
DO $staff_profile_owner_admin$
DECLARE visible_count integer;
BEGIN
    SELECT count(*) INTO visible_count FROM memberships;
    IF visible_count <> 4 THEN
        RAISE EXCEPTION 'Tenant-scoped owner/admin staff context should show all four active memberships, found %.', visible_count;
    END IF;
    SELECT count(*) INTO visible_count FROM person_identities;
    IF visible_count <> 4 THEN
        RAISE EXCEPTION 'Staff identity scope should expose only current and tenant-member identities, found %.', visible_count;
    END IF;
    INSERT INTO hosted_entity_images
        (tenant_id,entity_type,entity_id,content_type,byte_length,image_data,created_by_membership_id,updated_by_membership_id)
    VALUES
        ('10000000-0000-0000-0000-000000000001','staff','42000000-0000-0000-0000-000000000042','image/png',8,decode('89504e470d0a1a0a','hex'),'41000000-0000-0000-0000-000000000041','41000000-0000-0000-0000-000000000041');
    SELECT count(*) INTO visible_count FROM hosted_entity_images
    WHERE tenant_id='10000000-0000-0000-0000-000000000001' AND entity_type='staff'
      AND entity_id='42000000-0000-0000-0000-000000000042';
    IF visible_count <> 1 THEN
        RAISE EXCEPTION 'Tenant-scoped owner/admin could not retrieve the staff profile image.';
    END IF;
END
$staff_profile_owner_admin$;
SELECT set_config('pos.identity_id', '33000000-0000-0000-0000-000000000033', true),
       set_config('pos.membership_id', '43000000-0000-0000-0000-000000000043', true),
       set_config('pos.staff_roster_tenant_id', '', true);
DO $staff_profile_branch_admin$
DECLARE visible_count integer;
BEGIN
    SELECT count(*) INTO visible_count FROM memberships;
    IF visible_count <> 1 THEN
        RAISE EXCEPTION 'Branch-scoped admin must not read the tenant-wide staff roster.';
    END IF;
    SELECT count(*) INTO visible_count FROM hosted_entity_images
    WHERE tenant_id='10000000-0000-0000-0000-000000000001' AND entity_type='staff'
      AND entity_id='42000000-0000-0000-0000-000000000042';
    IF visible_count <> 0 THEN
        RAISE EXCEPTION 'Branch-scoped admin must not read tenant-wide staff profile images.';
    END IF;
    BEGIN
        INSERT INTO hosted_entity_images
            (tenant_id,entity_type,entity_id,content_type,byte_length,image_data,created_by_membership_id,updated_by_membership_id)
        VALUES
            ('10000000-0000-0000-0000-000000000001','staff','42000000-0000-0000-0000-000000000042','image/png',8,decode('89504e470d0a1a0a','hex'),'43000000-0000-0000-0000-000000000043','43000000-0000-0000-0000-000000000043');
        RAISE EXCEPTION 'Branch-scoped admin unexpectedly wrote a tenant-wide staff image.';
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'Branch-scoped admin cannot write tenant-wide staff images.';
    END;
END
$staff_profile_branch_admin$;
SELECT set_config('pos.identity_id', '30000000-0000-0000-0000-000000000003', true);
DO $tenant_access$
BEGIN
    IF (SELECT count(*) FROM memberships) <> 1 THEN
        RAISE EXCEPTION 'Runtime role did not see exactly its active identity membership.';
    END IF;
    IF (SELECT count(*) FROM tenants) <> 1 OR
       (SELECT display_name FROM tenants LIMIT 1) <> 'Tenant A' THEN
        RAISE EXCEPTION 'Tenant RLS exposed a missing or cross-tenant organization.';
    END IF;
    IF (SELECT count(*) FROM membership_role_grants) <> 1 THEN
        RAISE EXCEPTION 'Scoped role grant lookup returned the wrong rows.';
    END IF;
    IF (SELECT count(*) FROM tenant_entitlements) <> 1 OR
       (SELECT numeric_limit FROM tenant_entitlements WHERE entitlement_key = 'stations.active') <> 1 THEN
        RAISE EXCEPTION 'Runtime role did not see only the active tenant station entitlement.';
    END IF;
    IF (SELECT count(*) FROM branches) <> 1 OR
       (SELECT code FROM branches LIMIT 1) <> 'A' THEN
        RAISE EXCEPTION 'Branch RLS exposed a branch outside the membership grant.';
    END IF;
    IF (SELECT count(*) FROM stations) <> 1 OR
       (SELECT code FROM stations LIMIT 1) <> 'A1' THEN
        RAISE EXCEPTION 'Station RLS exposed a station outside the membership grant.';
    END IF;
END
$tenant_access$;
COMMIT;
DO $context_reset$
BEGIN
    IF (SELECT count(*) FROM memberships) <> 0 OR
       (SELECT count(*) FROM branches) <> 0 OR
       (SELECT count(*) FROM stations) <> 0 THEN
        RAISE EXCEPTION 'Transaction-local identity context leaked into the pooled connection.';
    END IF;
END
$context_reset$;
RESET ROLE;
SET ROLE pos_operator_runtime;
BEGIN;
SELECT set_config('pos.operator_issuer','https://operator.example',true),
       set_config('pos.operator_subject','operator-a',true);
DO $operator_boundary$
DECLARE operator_key uuid; visible_count integer; changed_count integer;
BEGIN
    SELECT id INTO operator_key FROM operator_accounts
    WHERE issuer='https://operator.example' AND subject='operator-a' AND status='active';
    IF operator_key IS NULL THEN RAISE EXCEPTION 'Provisioned operator identity did not resolve.'; END IF;
    PERFORM set_config('pos.operator_id',operator_key::text,true);
    SELECT count(*) INTO visible_count FROM hosted_payment_integration_requests;
    IF visible_count <> 1 THEN RAISE EXCEPTION 'Support operator queue did not see the expected request.'; END IF;
    UPDATE hosted_payment_integration_requests
       SET status='under_review',customer_visible_update='We are reviewing the request.',revision=revision+1,updated_at=now()
     WHERE id='b0000000-0000-0000-0000-000000000001' AND revision=1;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    IF changed_count <> 1 THEN RAISE EXCEPTION 'Support operator could not perform the request status update.'; END IF;
    INSERT INTO operator_payment_request_events(request_id,operator_id,event_kind,previous_status,new_status,customer_visible_update,resulting_revision)
      VALUES('b0000000-0000-0000-0000-000000000001',operator_key,'status_changed','submitted','under_review','We are reviewing the request.',2);
    INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id)
      VALUES(operator_key,'payment_request_updated','payment_integration_request','b0000000-0000-0000-0000-000000000001');
    IF (SELECT customer_visible_update FROM hosted_payment_integration_requests WHERE id='b0000000-0000-0000-0000-000000000001') <> 'We are reviewing the request.' THEN
       RAISE EXCEPTION 'Tenant-visible progress was not persisted.';
    END IF;
    BEGIN
        UPDATE hosted_payment_integration_requests SET details='changed' WHERE id='b0000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'Operator runtime changed tenant-submitted details.';
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'Operator runtime cannot alter tenant-submitted request details.';
    END;
    BEGIN
        DELETE FROM operator_payment_request_events;
        RAISE EXCEPTION 'Operator runtime deleted immutable request events.';
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'Operator runtime cannot delete request events.';
    END;
    BEGIN
        PERFORM id FROM memberships LIMIT 1;
        RAISE EXCEPTION 'Operator runtime unexpectedly read tenant membership data.';
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'Operator runtime cannot read tenant membership data.';
    END;
END
$operator_boundary$;
COMMIT;
RESET ROLE;
SET ROLE pos_operator_provisioner;
BEGIN;
DO $operator_provisioning$
DECLARE account_role text;
BEGIN
    SELECT role INTO account_role FROM operator_accounts
      WHERE issuer='https://operator.example' AND subject='operator-a';
    IF account_role <> 'support' THEN RAISE EXCEPTION 'Provisioner could not inspect its target account.'; END IF;
    UPDATE operator_accounts SET status='suspended' WHERE issuer='https://operator.example' AND subject='operator-a';
    INSERT INTO operator_provisioning_events(actor_label,action,target_operator_id,issuer,subject,role,status,reason)
      SELECT 'verifier','account_status_changed',id,issuer,subject,role,'suspended','Restricted provisioner verification.'
      FROM operator_accounts WHERE issuer='https://operator.example' AND subject='operator-a';
    IF (SELECT status FROM operator_accounts WHERE issuer='https://operator.example' AND subject='operator-a') <> 'suspended' THEN
       RAISE EXCEPTION 'Provisioner status change was not committed.';
    END IF;
END
$operator_provisioning$;
COMMIT;
RESET ROLE;
DO $block$
BEGIN
    INSERT INTO membership_role_grants (tenant_id, membership_id, role_key, branch_id)
    VALUES ('10000000-0000-0000-0000-000000000001', '40000000-0000-0000-0000-000000000004', 'cashier', '60000000-0000-0000-0000-000000000006');
    RAISE EXCEPTION 'Cross-tenant branch grant was unexpectedly accepted.';
EXCEPTION WHEN foreign_key_violation THEN
    RAISE NOTICE 'Cross-tenant branch grant correctly rejected.';
END
$block$;
'@ | Set-Content -LiteralPath $temporarySql -Encoding utf8

    $copy = Invoke-DockerCommand -Arguments @('cp', $temporarySql, "${containerName}:/tmp/tenant-boundary.sql")
    if ($copy.ExitCode -ne 0) { throw 'Could not copy tenant-boundary verification SQL.' }
    $runtimeGrantFile = Join-Path $repoRoot 'apps/api/docker/grant-runtime.sql'
    $runtimeGrantCopy = Invoke-DockerCommand -Arguments @('cp', $runtimeGrantFile, "${containerName}:/tmp/grant-runtime.sql")
    if ($runtimeGrantCopy.ExitCode -ne 0) { throw 'Could not copy the local Compose runtime grants into the verifier database.' }
    $sqlCheck = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-f', '/tmp/tenant-boundary.sql')
    if ($sqlCheck.ExitCode -ne 0) { throw "Tenant-boundary verification failed. $($sqlCheck.Error) $($sqlCheck.Output)" }

    $operatorProvisionerPassword = "provisioner-$([guid]::NewGuid().ToString('N'))"
    $operatorProvisionerLogin = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "ALTER ROLE pos_operator_provisioner LOGIN PASSWORD '$operatorProvisionerPassword' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;")
    if ($operatorProvisionerLogin.ExitCode -ne 0) { throw 'Could not create a disposable operator provisioner login.' }
    $env:POS_OPERATOR_PROVISIONING_DATABASE_URL = "postgres://pos_operator_provisioner:$operatorProvisionerPassword@127.0.0.1:$port/postgres?sslmode=disable"
    $env:POS_OPERATOR_PROVISIONING_ACTOR = 'migration-verifier'
    Push-Location $repoRoot
    try {
        $operatorExecutable = Join-Path $repoRoot 'target\debug\operator-api.exe'
        & $operatorExecutable provision https://operator.example operator-cli-verifier 'Verifier Operator' 'Provisioning CLI verifier account'
        if ($LASTEXITCODE -ne 0) { throw 'Operator provisioning command failed.' }
        $operatorIdResult = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-At', '-c', "SELECT id::text FROM operator_accounts WHERE issuer='https://operator.example' AND subject='operator-cli-verifier';")
        if ($operatorIdResult.ExitCode -ne 0 -or -not $operatorIdResult.Output.Trim()) { throw 'Operator provisioning command did not create an account.' }
        $operatorId = $operatorIdResult.Output.Trim()
        & $operatorExecutable set-status $operatorId suspended 'Verify support suspension'
        if ($LASTEXITCODE -ne 0) { throw 'Operator suspension command failed.' }
        & $operatorExecutable set-status $operatorId deactivated 'Verify support deactivation'
        if ($LASTEXITCODE -ne 0) { throw 'Operator deactivation command failed.' }
        & $operatorExecutable set-status $operatorId active 'Verify support reactivation'
        if ($LASTEXITCODE -ne 0) { throw 'Operator reactivation command failed.' }
        & $operatorExecutable provision https://operator.example operator-cli-verifier 'Verifier Operator' 'Duplicate must fail'
        if ($LASTEXITCODE -eq 0) { throw 'Operator provisioning accepted a duplicate issuer/subject.' }
    }
    finally {
        Pop-Location
    }
    $operatorProvisioningAudit = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-At', '-c', "SELECT status || ':' || (SELECT count(*) FROM operator_provisioning_events WHERE target_operator_id=operator_accounts.id)::text FROM operator_accounts WHERE issuer='https://operator.example' AND subject='operator-cli-verifier';")
    if ($operatorProvisioningAudit.ExitCode -ne 0 -or $operatorProvisioningAudit.Output.Trim() -ne 'active:4') { throw "Operator lifecycle audit verification failed: '$($operatorProvisioningAudit.Output.Trim())'." }

    $runtimeRole = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "CREATE ROLE pos_api_runtime_login LOGIN INHERIT PASSWORD '$runtimePassword' IN ROLE pos_api_runtime;")
    if ($runtimeRole.ExitCode -ne 0) { throw "Could not create a disposable restricted runtime login: $($runtimeRole.Error)" }
    $operatorRuntimeRole = Invoke-DockerCommand -Arguments @('exec', $containerName, 'psql', '-U', 'postgres', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', "CREATE ROLE pos_operator_runtime_login LOGIN PASSWORD '$runtimePassword' IN ROLE pos_operator_runtime;")
    if ($operatorRuntimeRole.ExitCode -ne 0) { throw "Could not create a disposable restricted operator login: $($operatorRuntimeRole.Error)" }
    @'
BEGIN;
SELECT set_config('pos.verified_issuer', 'https://issuer.example', true),
       set_config('pos.verified_subject', 'subject-a', true);
DO $runtime_login_identity$
BEGIN
    IF NOT pg_has_role(current_user, 'pos_api_runtime', 'USAGE') OR
       NOT has_column_privilege(current_user, 'pickup_reservation_events', 'event_type', 'SELECT') OR
       NOT has_column_privilege(current_user, 'pickup_reservation_events', 'reason', 'SELECT') OR
       (SELECT count(*) FROM person_identities) <> 1 OR
       (SELECT subject FROM person_identities LIMIT 1) <> 'subject-a' THEN
        RAISE EXCEPTION 'Restricted login did not inherit the required runtime privileges or verified identity scope.';
    END IF;
END
$runtime_login_identity$;
SELECT set_config('pos.identity_id', '30000000-0000-0000-0000-000000000003', true);
DO $runtime_login_tenant$
BEGIN
    IF (SELECT count(*) FROM tenants) <> 1 OR
       (SELECT display_name FROM tenants LIMIT 1) <> 'Tenant A' THEN
        RAISE EXCEPTION 'Restricted login saw a cross-tenant business.';
    END IF;
    IF (SELECT count(*) FROM branches) <> 1 OR
       (SELECT code FROM branches LIMIT 1) <> 'A' THEN
        RAISE EXCEPTION 'Restricted login saw a cross-tenant branch.';
    END IF;
    IF (SELECT count(*) FROM stations) <> 1 OR
       (SELECT code FROM stations LIMIT 1) <> 'A1' THEN
        RAISE EXCEPTION 'Restricted login saw a cross-tenant station.';
    END IF;
END
$runtime_login_tenant$;
COMMIT;
DO $runtime_login_reset$
BEGIN
    IF (SELECT count(*) FROM memberships) <> 0 OR
       (SELECT count(*) FROM branches) <> 0 OR
       (SELECT count(*) FROM stations) <> 0 THEN
        RAISE EXCEPTION 'Transaction-local identity context leaked after the restricted login committed.';
    END IF;
END
$runtime_login_reset$;
'@ | Set-Content -LiteralPath $temporarySql -Encoding utf8
    $copyRuntime = Invoke-DockerCommand -Arguments @('cp', $temporarySql, "${containerName}:/tmp/runtime-login.sql")
    if ($copyRuntime.ExitCode -ne 0) { throw 'Could not copy the restricted-login verification SQL.' }
    $loginCheck = Invoke-DockerCommand -Arguments @('exec', '-e', "PGPASSWORD=$runtimePassword", $containerName, 'psql', '-h', '127.0.0.1', '-U', 'pos_api_runtime_login', '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-f', '/tmp/runtime-login.sql')
    if ($loginCheck.ExitCode -ne 0) { throw "Restricted runtime login verification failed: $($loginCheck.Error) $($loginCheck.Output)" }

    $previousTestRuntimeUrl = $env:POS_API_TEST_DATABASE_URL
    $hadTestRuntimeUrl = Test-Path Env:POS_API_TEST_DATABASE_URL
    $previousTestSeedUrl = $env:POS_API_TEST_SEED_DATABASE_URL
    $hadTestSeedUrl = Test-Path Env:POS_API_TEST_SEED_DATABASE_URL
    $previousTestAttendanceWorkerUrl = $env:POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL
    $hadTestAttendanceWorkerUrl = Test-Path Env:POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL
    $previousOperatorTestRuntimeUrl = $env:POS_OPERATOR_TEST_DATABASE_URL
    $hadOperatorTestRuntimeUrl = Test-Path Env:POS_OPERATOR_TEST_DATABASE_URL
    try {
        $env:POS_API_TEST_DATABASE_URL = "postgres://pos_api_runtime_login:$runtimePassword@127.0.0.1:$port/postgres?sslmode=disable"
        $env:POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL = "postgres://pos_attendance_runner:$attendanceRunnerPassword@127.0.0.1:$port/postgres?sslmode=disable"
        $env:POS_OPERATOR_TEST_DATABASE_URL = "postgres://pos_operator_runtime_login:$runtimePassword@127.0.0.1:$port/postgres?sslmode=disable"
        $env:POS_API_TEST_SEED_DATABASE_URL = "postgres://postgres:$password@127.0.0.1:$port/postgres?sslmode=disable"
        Push-Location $repoRoot
        try {
            & (Join-Path $repoRoot 'tools\cargo-build.ps1') -Command test -CargoArgs @('-p', 'pos-api', '--lib', '--', '--ignored', '--nocapture', '--test-threads=1')
            if ($LASTEXITCODE -ne 0) { throw 'Restricted PostgreSQL HTTP integration tests failed.' }
        }
        finally {
            Pop-Location
        }
    }
    finally {
        if ($hadTestRuntimeUrl) { $env:POS_API_TEST_DATABASE_URL = $previousTestRuntimeUrl }
        else { Remove-Item Env:POS_API_TEST_DATABASE_URL -ErrorAction SilentlyContinue }
        if ($hadTestSeedUrl) { $env:POS_API_TEST_SEED_DATABASE_URL = $previousTestSeedUrl }
        else { Remove-Item Env:POS_API_TEST_SEED_DATABASE_URL -ErrorAction SilentlyContinue }
        if ($hadTestAttendanceWorkerUrl) { $env:POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL = $previousTestAttendanceWorkerUrl }
        else { Remove-Item Env:POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL -ErrorAction SilentlyContinue }
        if ($hadOperatorTestRuntimeUrl) { $env:POS_OPERATOR_TEST_DATABASE_URL = $previousOperatorTestRuntimeUrl }
        else { Remove-Item Env:POS_OPERATOR_TEST_DATABASE_URL -ErrorAction SilentlyContinue }
    }

    "All $expectedMigrationCount PostgreSQL migrations applied twice. Restricted tenant and operator HTTP scenarios passed, including supplier physical-return credits with stock/AP journal posting, exact retry and changed-payload rejection, over-allocation and balance guards, shared-ticket claim/save/checkout recovery, one committed shared sale with an exact retry from another station and no duplicate sale row, explicit shared-product candidates and branch attachment, branch profile authorization, commitment-aware transfer acceptance/dispatch, incremental receipt replay and discrepancy closure, guarded stock decreases, pickup hold creation/replay/conflict, ready/cancel transitions and queue scope, and the existing accounting and tenant scenarios."
}
finally {
    if ($hadMigrationUrl) {
        $env:POS_API_MIGRATION_DATABASE_URL = $previousMigrationUrl
    }
    else {
        Remove-Item Env:POS_API_MIGRATION_DATABASE_URL -ErrorAction SilentlyContinue
    }
    if ($hadOperatorProvisionUrl) { $env:POS_OPERATOR_PROVISIONING_DATABASE_URL = $previousOperatorProvisionUrl }
    else { Remove-Item Env:POS_OPERATOR_PROVISIONING_DATABASE_URL -ErrorAction SilentlyContinue }
    if ($hadOperatorProvisionActor) { $env:POS_OPERATOR_PROVISIONING_ACTOR = $previousOperatorProvisionActor }
    else { Remove-Item Env:POS_OPERATOR_PROVISIONING_ACTOR -ErrorAction SilentlyContinue }
    try {
        $containerState = Invoke-DockerCommand -Arguments @('container', 'inspect', $containerName) -TimeoutSeconds 10
        if ($preserveContainer) { Write-Warning "Preserving disposable PostgreSQL container for diagnosis: $containerName" }
        if (-not $preserveContainer -and $null -ne $containerState -and $containerState.ExitCode -eq 0) {
            $cleanup = Invoke-DockerCommand -Arguments @('rm', '-f', '-v', $containerName) -TimeoutSeconds 10
            if ($null -eq $cleanup -or $cleanup.ExitCode -ne 0) { Write-Warning 'Disposable PostgreSQL cleanup could not reach Docker. The container may need manual cleanup after the engine recovers.' }
        }
    }
    catch {
        Write-Warning 'Disposable PostgreSQL cleanup could not inspect Docker. Check for a leftover advanced-pos-pg-check container after the engine recovers.'
    }
    if (Test-Path -LiteralPath $temporarySql) {
        [System.IO.File]::Delete($temporarySql)
    }
}

