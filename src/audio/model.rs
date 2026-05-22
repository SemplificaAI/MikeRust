//! GGML model resolution for whisper.cpp.
//!
//! Resolution order (first match wins):
//! 1. `WHISPER_MODEL_PATH` env var — absolute or relative path to a
//!    specific `.bin` file. Lets advanced users point at a custom
//!    quantization or a model not on the bootstrap list.
//! 2. `<cache_dir>/<model_id>.bin`, where `<model_id>` defaults to
//!    `ggml-base` and is overridable via `WHISPER_MODEL`.
//! 3. None — caller surfaces a "first-use download required" status
//!    to the UI (the download itself is wired up at the model-status
//!    integration step; this module only resolves paths).
//!
//! The default cache directory mirrors fastembed (`%USERPROFILE%/
//! mikerust-data/whisper/` on Windows, `$HOME/.local/share/mikerust-data/
//! whisper/` on Linux, equivalent on macOS) so all the heavy local
//! model files live under one user-data tree and the Tauri watcher
//! never sees them.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Default GGML model. Multilingual, ~142 MB, decent quality on IT/EN
/// — the right starting point for legal / compliance audio (which is
/// what MikeRust actually transcribes). Users can swap to `tiny`
/// for latency or `small` / `medium` / `large` for quality via the
/// `WHISPER_MODEL` env var.
pub const DEFAULT_MODEL_ID: &str = "ggml-base";

/// Canonical mirror of the GGML weights. Used by the model-bootstrap
/// step (not yet wired in this module — just exported here so the
/// downloader stays in one place when it lands).
pub fn download_url(model_id: &str) -> String {
    format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}.bin",
        model_id
    )
}

/// Where MikeRust caches the GGML files. Created if missing.
pub fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var("MIKE_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs_user_data())
        .ok_or_else(|| anyhow!("cannot resolve user data directory for whisper cache"))?;
    let dir = base.join("whisper");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating whisper cache dir at {}", dir.display()))?;
    Ok(dir)
}

/// `~/mikerust-data/` on every platform — matches the convention used
/// by `FASTEMBED_CACHE_DIR` in the rest of the project.
fn dirs_user_data() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(PathBuf::from(home).join("mikerust-data"))
}

/// Resolve the GGML file to feed to whisper-rs. Returns `Ok(None)` when
/// no file is available on disk yet — caller decides whether to fail
/// the request or kick off a download.
pub fn resolve_model_path() -> Result<Option<PathBuf>> {
    // 1. Explicit override
    if let Ok(p) = std::env::var("WHISPER_MODEL_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(Some(path));
        }
        // Honour the override even when the file is missing: surface a
        // clear error rather than silently falling back to the cache.
        return Err(anyhow!(
            "WHISPER_MODEL_PATH points at non-existent file: {}",
            path.display()
        ));
    }

    // 2. Cache directory + WHISPER_MODEL (or default)
    let model_id =
        std::env::var("WHISPER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL_ID.to_string());
    let cache = cache_dir()?;
    let candidate = cache.join(format!("{model_id}.bin"));
    if candidate.is_file() {
        return Ok(Some(candidate));
    }

    // 3. Nothing on disk yet.
    Ok(None)
}

/// File the bootstrap downloader writes into. Always under `cache_dir()`,
/// always `<model_id>.bin` — separated from `resolve_model_path` so the
/// downloader has a stable target even when the user later sets
/// `WHISPER_MODEL_PATH` to something else.
pub fn cache_path_for(model_id: &str) -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{model_id}.bin")))
}

#[allow(dead_code)] // wired up at the model-status integration step
pub fn is_file(p: &Path) -> bool {
    p.is_file()
}
