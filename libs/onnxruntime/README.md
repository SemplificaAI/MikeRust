# onnxruntime — vendored runtime DLLs

This directory holds the **ONNX Runtime** native libraries that the
`ort` crate loads at runtime (we build with `ort/load-dynamic`, NOT
statically linked, NOT relying on a system installation). Drop the
right DLL/`.so`/`.dylib` for your target into the matching subfolder
and the embedding service will find it via the discovery walk in
[`src/embeddings/service.rs::ensure_onnxruntime_dylib_path`].

```
libs/onnxruntime/
├── win-x64/         onnxruntime.dll
├── win-arm64/       onnxruntime.dll
├── linux-x64/       libonnxruntime.so
├── linux-aarch64/   libonnxruntime.so
├── macos-x64/       libonnxruntime.dylib
└── macos-arm64/     libonnxruntime.dylib
```

The libraries themselves are **never committed to git** — see the
`*.dll` / `*.so` / `*.dylib` rules in the top-level `.gitignore`. Each
contributor / build runner fetches the variant they need for their
machine and target.

## Why local rather than system-installed?

- **Reproducibility.** The runtime version we test against is the
  one we ship. A user's `system32` may carry a different build with
  subtly different EP behaviour.
- **Sovereignty.** A sovereign-by-default product can't depend on
  whatever `onnxruntime.dll` happened to be installed by some
  upstream Windows update or NVIDIA toolchain.
- **Security.** No `LoadLibrary` against `system32` means no DLL-
  hijack surface where a poisoned `onnxruntime.dll` on the search
  path could be loaded before ours.

## Which variant?

ONNX Runtime ships several builds, each compiled with a different
set of execution providers (EPs). Pick the one that matches the
hardware on the deployment machine — every EP enabled in the Rust
build with `--features rag-<ep>` will be registered at session-init
time and silently skipped if the loaded DLL doesn't actually support
it. So you can build the binary with `rag-accel-all` and swap the
DLL freely depending on the host.

| Machine class | Recommended onnxruntime build | Bundle to download |
|---|---|---|
| **Windows desktop, any DX12 GPU** | DirectML | `Microsoft.ML.OnnxRuntime.DirectML` (NuGet) — onnxruntime.dll + DirectML.dll |
| **Windows + NVIDIA RTX/GeForce** | CUDA + TensorRT | `onnxruntime-win-x64-gpu-1.20.x.zip` (GitHub releases) |
| **Windows + Intel CPU/iGPU** | OpenVINO | `Microsoft.ML.OnnxRuntime.OpenVino` |
| **Windows ARM64 (Snapdragon X Elite)** | QNN + DirectML | `Microsoft.ML.OnnxRuntime.QNN` |
| **Linux + NVIDIA** | CUDA + TensorRT | `onnxruntime-linux-x64-gpu-1.20.x.tgz` |
| **Linux + AMD** | ROCm | `onnxruntime-linux-x64-rocm-1.20.x.tgz` |
| **macOS Apple Silicon** | CoreML | `onnxruntime-osx-arm64-1.20.x.tgz` |
| **macOS Intel** | CPU | `onnxruntime-osx-x86_64-1.20.x.tgz` |
| **CPU-only / portable** | base CPU | `onnxruntime-<os>-<arch>-1.20.x.{zip,tgz}` |

## Fetching (Windows quick path)

```powershell
# Pick the right URL from https://github.com/microsoft/onnxruntime/releases
# (replace 1.20.0 with the latest 1.20.x — ort 2.0.0-rc.12 targets the 1.x API)
$ver = "1.20.0"
$arch = "x64"   # or "arm64"
$url = "https://github.com/microsoft/onnxruntime/releases/download/v$ver/onnxruntime-win-$arch-$ver.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\ort.zip"
Expand-Archive "$env:TEMP\ort.zip" -DestinationPath "$env:TEMP\ort"
Copy-Item "$env:TEMP\ort\onnxruntime-win-$arch-$ver\lib\onnxruntime.dll" `
          -Destination ".\libs\onnxruntime\win-$arch\"
```

For DirectML / CUDA / OpenVINO / QNN variants, follow the same flow
with the corresponding NuGet or release artefact. The bundle ships
multiple DLLs (e.g. `onnxruntime.dll` + `DirectML.dll`) — copy *all*
of them into the same subdirectory. The loader picks up
`onnxruntime.dll` by name, but ort calls into the side-DLLs
internally at session time.

## Overriding the search path

Set `ORT_DYLIB_PATH` to an absolute path before the binary starts:

```powershell
$env:ORT_DYLIB_PATH = "C:\path\to\custom\onnxruntime.dll"
```

The `ensure_onnxruntime_dylib_path()` helper respects this and skips
the discovery walk. Useful for CI runners with a shared cache, or
for testing a specific build without copying it in-tree.

## When the DLL is missing

The service logs a warning at startup and the first embed call
fails with a clear "load library failed" error. The user-facing
catch is intentional: a silent fallback to a system DLL or to
"download on first run" would defeat the sovereignty guarantee
above.
