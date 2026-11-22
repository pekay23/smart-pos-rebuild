param(
    [ValidateSet('build', 'check', 'test', 'clippy', 'run', 'tauri-dev')]
    [string]$Command = 'build',
    [string[]]$CargoArgs = @(),
    [string]$ReleaseArtifactDirectory,
    [switch]$CleanupNow
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$targetDir = Join-Path $repoRoot 'target'
$nativeBuildDirs = @('build', 'build-msvc-x86', 'build-nmake', 'build-x86') | ForEach-Object {
    Join-Path $repoRoot $_
}
$maxBuildCommands = 100
$maxManagedOutputBytes = 21.5GB
$statePath = Join-Path $repoRoot '.planning\local-build-count.json'
if ($CargoArgs -contains '--target-dir' -or ($CargoArgs | Where-Object { $_ -like '--target-dir=*' })) {
    throw 'Do not override --target-dir through this wrapper; all builds must reuse the managed root target/ directory.'
}
if ($ReleaseArtifactDirectory -and $Command -notin @('build', 'run')) {
    throw 'ReleaseArtifactDirectory can only be used with a build or run command.'
}

function Read-BuildCount {
    if (-not (Test-Path -LiteralPath $statePath)) { return 0 }
    try { return [int](Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json).count }
    catch { throw "Local build counter is invalid at $statePath. Remove it after reviewing its contents." }
}

function Write-BuildCount([int]$Count) {
    $stateDirectory = Split-Path -Parent $statePath
    [void][System.IO.Directory]::CreateDirectory($stateDirectory)
    @{ count = $Count } | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding utf8
}

function Test-TargetInUse {
    $prefixes = @($targetDir) + $nativeBuildDirs | ForEach-Object { $_.TrimEnd('\') + '\' }
    return [bool](Get-CimInstance Win32_Process | Where-Object {
        $processPath = $_.ExecutablePath
        $processPath -and ($prefixes | Where-Object {
            $processPath.StartsWith($_, [System.StringComparison]::OrdinalIgnoreCase)
        } | Select-Object -First 1)
    } | Select-Object -First 1)
}

function Get-ManagedBuildSize {
    [long]$totalBytes = 0
    foreach ($directory in @($targetDir) + $nativeBuildDirs) {
        if (Test-Path -LiteralPath $directory -PathType Container) {
            $size = Get-ChildItem -LiteralPath $directory -Recurse -File -Force -ErrorAction SilentlyContinue |
                Measure-Object -Property Length -Sum
            if ($null -ne $size.Sum) { $totalBytes += [long]$size.Sum }
        }
    }
    return $totalBytes
}

function Clear-Target {
    if (Test-TargetInUse) {
        throw 'A local POS process is running from a managed build directory. Close it before managed build cleanup, then retry.'
    }
    & cargo clean --manifest-path (Join-Path $repoRoot 'Cargo.toml') --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { throw "Cargo cleanup failed with exit code $LASTEXITCODE." }
    foreach ($directory in $nativeBuildDirs) {
        if (Test-Path -LiteralPath $directory) {
            Remove-Item -LiteralPath $directory -Recurse -Force
        }
    }
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
}

if ($CleanupNow) {
    Clear-Target
    Write-Output 'Managed build outputs cleared; build counter reset.'
    exit 0
}

$buildCount = Read-BuildCount
if ($buildCount -ge $maxBuildCommands -or (Get-ManagedBuildSize) -ge $maxManagedOutputBytes) {
    Clear-Target
    $buildCount = 0
}

# Reserve the slot before starting Cargo. If PowerShell or Cargo is interrupted,
# the generated output still counts toward the command-count safety limit.
$buildCount++
Write-BuildCount $buildCount

    if ($Command -eq 'tauri-dev') {
        Push-Location (Join-Path $repoRoot 'apps\desktop')
    } else {
        Push-Location $repoRoot
    }
$previousTargetDir = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $targetDir
try {
        if ($Command -eq 'tauri-dev') {
            if ($CargoArgs.Count -gt 0) {
                throw 'CargoArgs are not accepted for tauri-dev; configure Tauri development options in the project.'
            }
            & npm run tauri -- dev
        } else {
            & cargo $Command --manifest-path (Join-Path $repoRoot 'Cargo.toml') @CargoArgs
        }
    $cargoExitCode = $LASTEXITCODE
} finally {
    if ($null -eq $previousTargetDir) { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
    else { $env:CARGO_TARGET_DIR = $previousTargetDir }
    Pop-Location
}

if ($buildCount -ge $maxBuildCommands -or (Get-ManagedBuildSize) -ge $maxManagedOutputBytes) {
    if ($cargoExitCode -eq 0 -and $ReleaseArtifactDirectory) {
        $releaseSource = Join-Path $targetDir 'release'
        $artifactRoot = [System.IO.Path]::GetFullPath($ReleaseArtifactDirectory)
        $repoPrefix = [System.IO.Path]::TrimEndingDirectorySeparator($repoRoot) + [System.IO.Path]::DirectorySeparatorChar
        if ($artifactRoot.Equals([System.IO.Path]::TrimEndingDirectorySeparator($repoRoot), [System.StringComparison]::OrdinalIgnoreCase) -or $artifactRoot.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or $artifactRoot.StartsWith([System.IO.Path]::TrimEndingDirectorySeparator($targetDir) + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw 'ReleaseArtifactDirectory must be outside the repository and managed target directory.'
        }
        $executable = Join-Path $releaseSource 'smart-pos-desktop.exe'
        if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
            throw "The expected desktop release executable was not produced: '$executable'."
        }
        [void][System.IO.Directory]::CreateDirectory($artifactRoot)
        Get-ChildItem -LiteralPath $releaseSource -File | Where-Object { $_.Extension -in @('.exe', '.dll') } | Copy-Item -Destination $artifactRoot -Force
        Write-Output "Release executables and DLLs copied to '$artifactRoot' before managed build cleanup."
    }
    try { Clear-Target }
    catch { Write-Warning "Cargo $Command completed, but cleanup is pending: $($_.Exception.Message)" }
}
Write-Output "Build command '$Command' finished. Build counter is $buildCount of $maxBuildCommands; managed output cleanup runs at 21.5 GiB or the command-count limit."
if ($cargoExitCode -ne 0) { exit $cargoExitCode }
