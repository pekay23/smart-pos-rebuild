# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Powered Solutions

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string] $PackDirectory,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+([.-][0-9A-Za-z.-]+)?$')]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string] $MainExe,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ $_ -match '^https://' })]
    [string] $FeedUrl,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string] $OutputDirectory,

    [ValidateSet('demo', 'beta', 'stable')]
    [string] $Channel = 'stable',

    [string] $ReleaseNotes,

    [switch] $AllowInitialRelease
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnet) {
    throw 'The .NET 8 SDK or later is required to restore the repository-pinned Velopack CLI.'
}
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$toolManifest = Join-Path $repoRoot '.config/dotnet-tools.json'
if (-not (Test-Path -LiteralPath $toolManifest -PathType Leaf)) {
    throw "Pinned Velopack CLI manifest was not found: '$toolManifest'."
}

$resolvedPackDirectory = (Resolve-Path -LiteralPath $PackDirectory).Path
$resolvedOutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
$packRoot = [System.IO.Path]::TrimEndingDirectorySeparator($resolvedPackDirectory)
$outputRoot = [System.IO.Path]::TrimEndingDirectorySeparator($resolvedOutputDirectory)
if ($outputRoot.Equals($packRoot, [System.StringComparison]::OrdinalIgnoreCase) -or $outputRoot.StartsWith($packRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'OutputDirectory must be outside PackDirectory so release artifacts are not accidentally included in the next application package.'
}
if (-not (Test-Path -LiteralPath (Join-Path $resolvedPackDirectory $MainExe) -PathType Leaf)) {
    throw "Main executable '$MainExe' was not found in '$resolvedPackDirectory'."
}
if ($ReleaseNotes -and -not (Test-Path -LiteralPath $ReleaseNotes -PathType Leaf)) {
    throw "Release notes file '$ReleaseNotes' was not found."
}
$resolvedReleaseNotes = if ($ReleaseNotes) { (Resolve-Path -LiteralPath $ReleaseNotes).Path } else { $null }

New-Item -ItemType Directory -Force -Path $resolvedOutputDirectory | Out-Null
$existingTargetPackage = Join-Path $resolvedOutputDirectory "PoweredSolutions.AdvancedPOS-$Version-$Channel-full.nupkg"
if (Test-Path -LiteralPath $existingTargetPackage) {
    throw "Version $Version already has a package in the output directory. Releases are immutable; use a new version and a clean output directory."
}

Push-Location $repoRoot
try {
    & $dotnet.Source tool run vpk -- -H *> $null
    if ($LASTEXITCODE -ne 0) {
        & $dotnet.Source tool restore --tool-manifest $toolManifest
        if ($LASTEXITCODE -ne 0) { throw "Could not restore the pinned Velopack CLI (exit code $LASTEXITCODE)." }
    }

    function Invoke-PinnedVpk([string[]] $Arguments) {
        & $dotnet.Source tool run vpk -- @Arguments
        if ($LASTEXITCODE -ne 0) { throw "Velopack command failed (exit code $LASTEXITCODE): $($Arguments -join ' ')" }
    }

$downloadArgs = @('download', 'http', '--url', $FeedUrl, '--channel', $Channel, '--outputDir', $resolvedOutputDirectory, '--allowEmptyChannel')
Invoke-PinnedVpk $downloadArgs

$previousPackage = Get-ChildItem -LiteralPath $resolvedOutputDirectory -Filter "PoweredSolutions.AdvancedPOS-*-$Channel-full.nupkg" -File |
    Where-Object { $_.Name -notmatch "PoweredSolutions\.AdvancedPOS-$([regex]::Escape($Version))-$([regex]::Escape($Channel))-full\.nupkg$" } |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $previousPackage -and -not $AllowInitialRelease) {
    throw "No previous full release was downloaded for '$Channel'. Refusing to publish a regular release without a base package for delta generation. Use -AllowInitialRelease only for a new channel or the one-time migration release."
}

$packArgs = @(
    'pack', '--packId', 'PoweredSolutions.AdvancedPOS', '--packVersion', $Version,
    '--packDir', $resolvedPackDirectory, '--outputDir', $resolvedOutputDirectory,
    '--channel', $Channel, '--packTitle', 'Advanced POS', '--mainExe', $MainExe,
    '--delta', 'BestSize'
)
if ($resolvedReleaseNotes) { $packArgs += @('--releaseNotes', $resolvedReleaseNotes) }
Invoke-PinnedVpk $packArgs

$fullPackage = Join-Path $resolvedOutputDirectory "PoweredSolutions.AdvancedPOS-$Version-$Channel-full.nupkg"
if (-not (Test-Path -LiteralPath $fullPackage -PathType Leaf)) {
    throw "Velopack did not produce the expected full update package '$fullPackage'."
}
if ($previousPackage) {
    $deltaPattern = "PoweredSolutions.AdvancedPOS-$Version-$Channel-delta.nupkg"
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedOutputDirectory $deltaPattern) -PathType Leaf)) {
        throw "A previous release exists, but no delta package was generated. Verify the vpk CLI matches the pinned Rust updater version and inspect packaging output before publishing."
    }
}

Write-Host "Release packages are ready in '$resolvedOutputDirectory'."
Write-Host 'Publish the complete channel feed and all referenced immutable assets together; do not replace or remove older delta packages still needed by clients.'
}
finally {
    Pop-Location
}
