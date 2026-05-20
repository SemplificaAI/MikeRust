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

  Cross-toolchain: each build is launched through the matching
  `VsDevCmd.bat -arch=<target> -host_arch=<host>` so the right
  `cl.exe` / `link.exe` are on PATH regardless of how the caller's
  shell was opened. The script auto-discovers Visual Studio via
  `vswhere.exe` and fails fast with the exact workload id if the
  required MSVC component isn't installed.

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
# VsDevCmd's `-arch` argument name (x64 is 'amd64' in VsDevCmd-speak).
$vsArchByArch = @{
    'x64'   = 'amd64'
    'arm64' = 'arm64'
}
# The MSVC workload that ships the C/C++ build tools for each target.
# vswhere will reject a VS install missing the required component, so we
# surface a clear error pointing at the VS Installer instead of letting
# link.exe fail with an opaque "machine type" message later.
$workloadByArch = @{
    'x64'   = 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
    'arm64' = 'Microsoft.VisualStudio.Component.VC.Tools.ARM64'
}
$archesToBuild = if ($Target -eq 'both') { @('x64', 'arm64') } else { @($Target) }

function Get-HostArch {
    switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
        'Arm64' { return 'arm64' }
        'X64'   { return 'amd64' }
        'X86'   { return 'x86' }
        default { throw "Unsupported host architecture: $_" }
    }
}

function Get-VsInstallPath {
    [CmdletBinding()]
    param([string[]]$Requires)
    $vswhere = @(
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\Installer\vswhere.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $vswhere) {
        throw 'vswhere.exe not found. Install Visual Studio Build Tools 2022 (or newer) from https://visualstudio.microsoft.com/downloads/.'
    }
    $vswhereArgs = @('-latest', '-products', '*', '-property', 'installationPath')
    foreach ($req in $Requires) { $vswhereArgs += @('-requires', $req) }
    $path = & $vswhere @vswhereArgs
    if (-not $path) {
        throw ("No Visual Studio installation found with required component(s): {0}. " +
               'Open Visual Studio Installer and add the missing workload.') -f ($Requires -join ', ')
    }
    return ($path | Select-Object -First 1).Trim()
}

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

$hostArch = Get-HostArch
Write-Host ("Host architecture: {0}" -f $hostArch) -ForegroundColor DarkGray

$built = @()
foreach ($arch in $archesToBuild) {
    $triple = $tripleByArch[$arch]
    Write-Host ''
    Write-Host "=== Build $arch ($triple) ===" -ForegroundColor Cyan

    # 1. Locate the Visual Studio install that carries the C++ build
    #    tools for this specific arch. Fails with a clear error if the
    #    matching workload isn't installed — much better than letting
    #    link.exe blow up later on a "machine type" mismatch.
    $vsInstall = Get-VsInstallPath -Requires @($workloadByArch[$arch])
    $vsDev = Join-Path $vsInstall 'Common7\Tools\VsDevCmd.bat'
    if (-not (Test-Path $vsDev)) {
        throw "VsDevCmd.bat not found at $vsDev (Visual Studio at $vsInstall)."
    }
    Write-Host ("VS install : {0}" -f $vsInstall) -ForegroundColor DarkGray
    Write-Host ("VsDevCmd   : {0} -arch={1} -host_arch={2}" -f $vsDev, $vsArchByArch[$arch], $hostArch) -ForegroundColor DarkGray

    $bundleRoot = Join-Path $RepoRoot "target\$triple\release\bundle"
    if ($Clean -and (Test-Path $bundleRoot)) {
        Write-Host "Cleaning $bundleRoot" -ForegroundColor DarkGray
        Remove-Item -Recurse -Force $bundleRoot
    }

    $overlay = New-ResourcesOverlay -Arch $arch
    Write-Host "Overlay    : $overlay" -ForegroundColor DarkGray

    # 2. Build the cmd-line we hand off to `cmd /c`. VsDevCmd.bat
    #    primes PATH/INCLUDE/LIB/LIBPATH for the right cross-target,
    #    then `tauri.CMD build` inherits them. Each arch gets a fresh
    #    cmd subprocess so the env never leaks between iterations.
    #    --bundles msi overrides the conf's `bundle.targets: "all"`
    #    to skip the NSIS .exe; the base + overlay configs together
    #    produce a single MSI with the matching arch DLLs bundled
    #    into <install>/resources/libs/<lib>/win-<arch>/.
    $cmdLine = '"{0}" -arch={1} -host_arch={2} -no_logo && "{3}" build --config "{4}" --config "{5}" --target {6} --bundles msi' -f `
        $vsDev, $vsArchByArch[$arch], $hostArch, $tauriBin, $config, $overlay, $triple

    & cmd.exe /c $cmdLine
    if ($LASTEXITCODE -ne 0) {
        throw "tauri build failed for $triple (exit $LASTEXITCODE). Check the cl.exe/link.exe output above for the failing step."
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
