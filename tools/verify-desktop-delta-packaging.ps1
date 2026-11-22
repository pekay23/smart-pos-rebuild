# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Powered Solutions

[CmdletBinding()]
param(
    [switch] $KeepArtifacts
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnet) { throw 'The .NET SDK is required to run the pinned Velopack packaging drill.' }
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$toolManifest = Join-Path $repoRoot '.config/dotnet-tools.json'
$manifest = Get-Content -LiteralPath $toolManifest -Raw | ConvertFrom-Json
$pinnedVersion = [string]$manifest.tools.vpk.version
if (-not $pinnedVersion) { throw 'The repository does not pin the Velopack CLI version.' }
$channel = 'stable'

$tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = Join-Path $tempParent "advanced-pos-delta-drill-$([guid]::NewGuid().ToString('N'))"
$releaseDirectory = Join-Path $testRoot 'releases'
$baseDirectory = Join-Path $testRoot 'app-v1'
$targetDirectory = Join-Path $testRoot 'app-v2'
$windowsExe = Join-Path $env:WINDIR 'System32\where.exe'
$completed = $false

function Invoke-PinnedVpk([string[]] $Arguments) {
    & $dotnet.Source tool run vpk -- @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Pinned Velopack $pinnedVersion command failed (exit $LASTEXITCODE): $($Arguments -join ' ')" }
}

try {
    if (-not (Test-Path -LiteralPath $windowsExe -PathType Leaf)) {
        throw "A local Windows PE file is required for the package fixture: '$windowsExe'."
    }
    [void][System.IO.Directory]::CreateDirectory($releaseDirectory)
    [void][System.IO.Directory]::CreateDirectory($baseDirectory)
    Copy-Item -LiteralPath $windowsExe -Destination (Join-Path $baseDirectory 'AdvancedPOS.exe')
    [System.IO.File]::WriteAllText((Join-Path $baseDirectory 'build-info.json'), '{"version":"1.0.0"}')

    $payload = [byte[]]::new(2MB)
    [System.Random]::new(174203).NextBytes($payload)
    [System.IO.File]::WriteAllBytes((Join-Path $baseDirectory 'catalog-cache.bin'), $payload)

    $commonPackArgs = @(
        'pack', '--packId', 'PoweredSolutions.AdvancedPOS', '--packTitle', 'Advanced POS delta drill',
        '--channel', $channel, '--mainExe', 'AdvancedPOS.exe', '--delta', 'BestSize',
        '--outputDir', $releaseDirectory
    )
    Invoke-PinnedVpk ($commonPackArgs + @('--packVersion', '1.0.0', '--packDir', $baseDirectory))
    $basePackage = Join-Path $releaseDirectory "PoweredSolutions.AdvancedPOS-1.0.0-$channel-full.nupkg"
    if (-not (Test-Path -LiteralPath $basePackage -PathType Leaf)) {
        throw "The pinned packager did not produce the expected base package '$basePackage'."
    }

    Copy-Item -LiteralPath $baseDirectory -Destination $targetDirectory -Recurse
    $targetInfoPath = Join-Path $targetDirectory 'build-info.json'
    [System.IO.File]::WriteAllText($targetInfoPath, '{"version":"1.0.1"}')
    $targetPayloadPath = Join-Path $targetDirectory 'catalog-cache.bin'
    $targetPayload = [System.IO.File]::ReadAllBytes($targetPayloadPath)
    for ($index = 786432; $index -lt 819200; $index++) { $targetPayload[$index] = $targetPayload[$index] -bxor 0xff }
    [System.IO.File]::WriteAllBytes($targetPayloadPath, $targetPayload)

    Invoke-PinnedVpk ($commonPackArgs + @('--packVersion', '1.0.1', '--packDir', $targetDirectory))
    $targetPackage = Join-Path $releaseDirectory "PoweredSolutions.AdvancedPOS-1.0.1-$channel-full.nupkg"
    $deltaPackage = Join-Path $releaseDirectory "PoweredSolutions.AdvancedPOS-1.0.1-$channel-delta.nupkg"
    if (-not (Test-Path -LiteralPath $targetPackage -PathType Leaf)) {
        throw "The pinned packager did not produce the expected target package '$targetPackage'."
    }
    if (-not (Test-Path -LiteralPath $deltaPackage -PathType Leaf)) {
        throw 'The pinned packager did not produce a delta for the second release.'
    }

    $targetSize = (Get-Item -LiteralPath $targetPackage).Length
    $deltaSize = (Get-Item -LiteralPath $deltaPackage).Length
    if ($deltaSize -ge $targetSize) {
        throw "BestSize delta ($deltaSize bytes) is not smaller than the full target package ($targetSize bytes)."
    }

    $reconstructedPackage = Join-Path $testRoot 'reconstructed-v2-full.nupkg'
    Invoke-PinnedVpk (@('delta', 'patch', '--base', $basePackage, '--patch', $deltaPackage, '--output', $reconstructedPackage))
    $expectedHash = (Get-FileHash -LiteralPath $targetPackage -Algorithm SHA256).Hash
    $actualHash = (Get-FileHash -LiteralPath $reconstructedPackage -Algorithm SHA256).Hash
    if ($actualHash -ne $expectedHash) {
        throw "The delta reconstruction differs from the v2 package (expected $expectedHash; got $actualHash)."
    }

    $missingOutput = Join-Path $testRoot 'missing-delta-output.nupkg'
    $missingPatchPath = Join-Path $releaseDirectory 'does-not-exist-delta.nupkg'
    $missingResult = & $dotnet.Source tool run vpk -- delta patch --base $basePackage --patch $missingPatchPath --output $missingOutput 2>&1
    $missingExitCode = $LASTEXITCODE
    if ($missingExitCode -eq 0 -or (Test-Path -LiteralPath $missingOutput)) {
        throw "A missing delta was not rejected cleanly (exit $missingExitCode): $($missingResult -join ' ')"
    }

    $corruptDeltaPath = Join-Path $testRoot 'corrupt-v2-delta.nupkg'
    $corruptOutput = Join-Path $testRoot 'corrupt-delta-output.nupkg'
    $deltaBytes = [System.IO.File]::ReadAllBytes($deltaPackage)
    if ($deltaBytes.Length -lt 16) { throw 'The generated delta is too small for the corruption check.' }
    [System.IO.File]::WriteAllBytes($corruptDeltaPath, $deltaBytes[0..([Math]::Floor($deltaBytes.Length / 2) - 1)])
    $corruptResult = & $dotnet.Source tool run vpk -- delta patch --base $basePackage --patch $corruptDeltaPath --output $corruptOutput 2>&1
    $corruptExitCode = $LASTEXITCODE
    if ($corruptExitCode -eq 0 -or (Test-Path -LiteralPath $corruptOutput)) {
        throw "A truncated delta was not rejected cleanly (exit $corruptExitCode): $($corruptResult -join ' ')"
    }

    $completed = $true
    Write-Output "Pinned Velopack $pinnedVersion delta packaging passed."
    Write-Output "Transferred delta: $deltaSize bytes; target full package (not transferred): $targetSize bytes."
    Write-Output "Reconstructed SHA-256: $actualHash."
    Write-Output 'Missing and truncated delta packages were rejected without producing output packages.'
    if ($KeepArtifacts) { Write-Output "Review artifacts: $testRoot" }
}
finally {
    if ($completed -and -not $KeepArtifacts -and (Test-Path -LiteralPath $testRoot -PathType Container)) {
        $resolvedRoot = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $testRoot).Path)
        $resolvedParent = [System.IO.Path]::TrimEndingDirectorySeparator([System.IO.Path]::GetFullPath((Split-Path -Parent $resolvedRoot)))
        $rootName = Split-Path -Leaf $resolvedRoot
        $expectedParent = [System.IO.Path]::TrimEndingDirectorySeparator([System.IO.Path]::GetFullPath($tempParent))
        if (-not $resolvedParent.Equals($expectedParent, [System.StringComparison]::OrdinalIgnoreCase) -or $rootName -notmatch '^advanced-pos-delta-drill-[0-9a-f]{32}$') {
            throw "Refusing to remove an unexpected packaging-drill path: '$resolvedRoot'."
        }
        [System.IO.Directory]::Delete($resolvedRoot, $true)
    }
    elseif (-not $completed) {
        Write-Warning "Packaging drill failed; its isolated artifacts were retained at '$testRoot'."
    }
}
