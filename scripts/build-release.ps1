# Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.
#requires -Version 5.1
<#
.SYNOPSIS
  Build MikeRust release MSI installers for Windows x86_64 and ARM64,
  bundle the matching native DLLs (onnxruntime + pdfium), and collect
  the artefacts under ./dist/.

.DESCRIPTION
  Pipeline per target architecture:
    1. Make sure the native runtime DLLs for the target arch exist by
       running scripts/fetch-native-libs.ps1 (idempotent — skips DLLs
       already on disk).
    2. Hand-craft a per-arch JSON overlay for `bundle.resources` that
       names only the matching arch's DLLs and write it under
       target/.tauri-overlay-<arch>.json.
    3. Invoke `tauri build --bundles msi --target <triple>` with both
       the base config and the overlay so the WiX bundler picks up the
       correct DLLs (and only those, not double-sized cross-arch).
    4. Copy the produced .msi from
       `target/<triple>/release/bundle/msi/` into ./dist/, renaming
       with a short `_x64` / `_arm64` suffix to keep both architectures
       coexisting.

  No manual operator step needed: a clean checkout can do
  `pnpm --dir frontend install` then `./scripts/build-release.ps1` and
  end up with two MSIs in dist/, each carrying the right native DLLs
  next to the binary.

  Cross-toolchain note: building both archs on the same Windows host
  requires the corresponding MSVC build tools installed via the Visual
  Studio Installer ("MSVC v143 - VS 2022 C++ x64/x86" and
  "MSVC v143 - VS 2022 C++ ARM64/ARM64EC"). The script does not
  check for those — `link.exe` failures with a "machine type" message
  point at this if you see them.

.PARAMETER Target
  Which architecture(s) to build. Defaults to `both`.
.PARAMETER Clean
  Wipe `target/<triple>/release/bundle/` before each build.
.PARAMETER FrontendInstall
  Run `pnpm --dir frontend install --frozen-lockfile` before building.
  Useful in CI; in a developer checkout it's usually redundant.
.PARAMETER SkipNativeLibs
  Skip the auto fetch-native-libs.ps1 invocation. Useful if you have
  already pinned a custom DLL build into libs/ that you don't want
  overwritten.

.EXAMPLE
  ./scripts/build-release.ps1                          # both, fresh DLLs as needed
  ./scripts/build-release.ps1 -Target x64 -Clean
  ./scripts/build-release.ps1 -Target arm64 -SkipNativeLibs
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64', 'both')]
    [string]$Target = 'both',
    [switch]$Clean,
    [switch]$FrontendInstall,
    [switch]$SkipNativeLibs
)

$ErrorActionPreference = 'Stop'

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

# Pre-flight: every requested arch must have its rustc target installed.
$installedTargets = (rustup target list --installed) -split "`n" | ForEach-Object { $_.Trim() }
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    if ($installedTargets -notcontains $triple) {
        throw "Rust target $triple is not installed. Run ``rustup target add $triple``."
    }
}

# Auto-fetch native libs for any requested arch that doesn't have them.
if (-not $SkipNativeLibs) {
    $needsFetch = @()
    foreach ($arch in $archesToBuild) {
        $onnxDll   = "libs\onnxruntime\win-$arch\onnxruntime.dll"
        $pdfiumDll = "libs\pdfium\win-$arch\pdfium.dll"
        if (-not (Test-Path $onnxDll) -or -not (Test-Path $pdfiumDll)) {
            $needsFetch += $arch
        }
    }
    if ($needsFetch.Count -gt 0) {
        Write-Host ('=== Fetching native DLLs for: {0} ===' -f ($needsFetch -join ', ')) -ForegroundColor Cyan
        foreach ($arch in $needsFetch) {
            & (Join-Path $PSScriptRoot 'fetch-native-libs.ps1') -Arch $arch
            if ($LASTEXITCODE -ne 0) {
                throw "fetch-native-libs.ps1 failed for $arch (exit $LASTEXITCODE)"
            }
        }
    } else {
        Write-Host 'Native DLLs already present for all requested arches.' -ForegroundColor DarkGray
    }
}

# Per-arch bundle.resources overlay. Tauri 2 takes --config multiple
# times; the second --config overlays the first. Writing the overlay
# to a file avoids any cmd/powershell quote-escaping pitfalls.
$overlayDir = Join-Path $RepoRoot 'target'
if (-not (Test-Path $overlayDir)) {
    New-Item -ItemType Directory -Path $overlayDir -Force | Out-Null
}
function New-ResourcesOverlay {
    param([string]$Arch)
    # Map syntax: "<source-relative-to-repo-root>" -> "<dest-relative-to-resources/>".
    # WiX MSI installer copies the destinations under <install>/resources/.
    # The Rust loaders already check `<exe_dir>/resources/libs/...`,
    # so the dev and install layouts converge.
    $resources = [ordered]@{
        ("libs/pdfium/win-$Arch/pdfium.dll")             = "libs/pdfium/win-$Arch/pdfium.dll"
        ("libs/onnxruntime/win-$Arch/onnxruntime.dll")   = "libs/onnxruntime/win-$Arch/onnxruntime.dll"
    }
    $obj = @{ bundle = @{ resources = $resources } }
    $path = Join-Path $overlayDir ("tauri-overlay-$Arch.json")
    # PowerShell 5.1's ConvertTo-Json defaults to depth 2 — passes here
    # because the structure is shallow; bump it for safety.
    $obj | ConvertTo-Json -Depth 6 | Set-Content -Path $path -Encoding UTF8
    return $path
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

    $overlay = New-ResourcesOverlay -Arch $arch
    Write-Host "Overlay: $overlay" -ForegroundColor DarkGray

    # --bundles msi overrides the conf's `bundle.targets: "all"` so we
    # skip the NSIS .exe. The base config plus the overlay together
    # produce a single MSI with the matching arch DLLs bundled into
    # <install>/resources/libs/<lib>/win-<arch>/.
    & $tauriBin build `
        --config $config `
        --config $overlay `
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
        # Tauri names MSIs `<Product>_<Version>_<arch>_<locale>.msi`.
        # Drop the locale tail and stamp our short arch label so x64 and
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
