# Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Build MikeRust release MSI installers for Windows x86_64 and ARM64
  and collect them under ./dist/.

.DESCRIPTION
  Drives `tauri build` once per target triple (`x86_64-pc-windows-msvc`
  and `aarch64-pc-windows-msvc`). The frontend bundle is produced by
  Tauri's beforeBuildCommand (`pnpm --dir ./frontend build`) — no
  manual prep needed. The produced .msi files are copied into the
  top-level `dist/` directory, renamed with the architecture suffix
  so x64 and ARM64 artefacts coexist without colliding.

  CAVEAT — native runtime DLLs.
  The runtime needs `onnxruntime.dll` and `pdfium.dll` next to the
  installed binary for the RAG and PDF paths to work. These are NOT
  yet bundled via `tauri.bundle.resources` in
  `src-tauri/tauri.svelte.conf.json`, so the MSI installs only the
  application binary. Until that bundling lands, post-install you must
  drop the matching DLLs into the install folder by hand — see
  `libs/onnxruntime/README.md` for the version that pairs with the
  current `ort` pin.

.PARAMETER Target
  Which architecture(s) to build. Defaults to `both`.
.PARAMETER Clean
  Wipe `target/<triple>/release/bundle/` before each build so a stale
  MSI from a previous run can't be picked up by mistake.
.PARAMETER FrontendInstall
  Run `pnpm --dir frontend install --frozen-lockfile` before building.
  Useful in CI; in a developer checkout it's usually redundant.

.EXAMPLE
  ./scripts/build-release.ps1                  # both architectures
  ./scripts/build-release.ps1 -Target x64      # just x86_64
  ./scripts/build-release.ps1 -Target arm64 -Clean
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64', 'both')]
    [string]$Target = 'both',
    [switch]$Clean,
    [switch]$FrontendInstall
)

$ErrorActionPreference = 'Stop'

# Repo root = parent of the scripts/ folder this file lives in.
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$tauriBin = Join-Path $RepoRoot 'frontend\node_modules\.bin\tauri.CMD'
if (-not (Test-Path $tauriBin)) {
    throw "Local tauri CLI not found at $tauriBin. Run ``pnpm --dir frontend install`` first."
}

$config = 'src-tauri\tauri.svelte.conf.json'
if (-not (Test-Path $config)) {
    throw "Tauri config not found at $config. Are you running from the repo root?"
}

$distDir = Join-Path $RepoRoot 'dist'
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}

if ($FrontendInstall) {
    Write-Host '=== pnpm install (frontend) ===' -ForegroundColor Cyan
    pnpm --dir frontend install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed (exit $LASTEXITCODE)" }
}

$tripleByArch = @{
    'x64'   = 'x86_64-pc-windows-msvc'
    'arm64' = 'aarch64-pc-windows-msvc'
}
$archesToBuild = if ($Target -eq 'both') { @('x64', 'arm64') } else { @($Target) }

# Verify the rustc target is installed before kicking off a long build.
$installedTargets = (rustup target list --installed) -split "`n" | ForEach-Object { $_.Trim() }
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    if ($installedTargets -notcontains $triple) {
        throw "Rust target $triple is not installed. Run ``rustup target add $triple``."
    }
}

$built = @()
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    Write-Host ''
    Write-Host "=== Build $arch ($triple) ===" -ForegroundColor Cyan

    $bundleRoot = Join-Path $RepoRoot "target\$triple\release\bundle"
    if ($Clean -and (Test-Path $bundleRoot)) {
        Write-Host "Cleaning $bundleRoot" -ForegroundColor DarkGray
        Remove-Item -Recurse -Force $bundleRoot
    }

    # --bundles msi overrides the conf's `bundle.targets: "all"` so we
    # don't waste time also producing the NSIS .exe installer.
    & $tauriBin build `
        --config $config `
        --target $triple `
        --bundles msi
    if ($LASTEXITCODE -ne 0) {
        throw "tauri build failed for $triple (exit $LASTEXITCODE)"
    }

    $msiSrcDir = Join-Path $bundleRoot 'msi'
    if (-not (Test-Path $msiSrcDir)) {
        throw "Expected MSI output directory not found: $msiSrcDir"
    }
    $msiFiles = Get-ChildItem -Path $msiSrcDir -Filter '*.msi' -File
    if ($msiFiles.Count -eq 0) {
        throw "tauri build for $triple produced no .msi in $msiSrcDir"
    }
    foreach ($msi in $msiFiles) {
        # Tauri names MSIs `<Product>_<Version>_<arch>_<locale>.msi`. We
        # drop the locale tail and stamp our short arch label so x64 and
        # arm64 artefacts never collide in dist/.
        $stem  = [IO.Path]::GetFileNameWithoutExtension($msi.Name)
        $clean = $stem -replace '_[a-z]{2}-[A-Z]{2}$', '' `
                       -replace '_(x64|arm64|x86|aarch64)$', ''
        $destName = "${clean}_$arch.msi"
        $dest = Join-Path $distDir $destName
        Copy-Item -Path $msi.FullName -Destination $dest -Force
        Write-Host ("  -> dist\{0}" -f $destName) -ForegroundColor Green
        $built += $dest
    }
}

Write-Host ''
Write-Host '=== Done ===' -ForegroundColor Green
Get-ChildItem -Path $distDir -Filter '*.msi' -File |
    Sort-Object Name |
    Format-Table @{n='File';e={$_.Name}}, @{n='Size (MB)';e={[math]::Round($_.Length / 1MB, 1)}}, LastWriteTime -AutoSize
