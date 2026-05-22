//! GGML model bootstrap — first-use download from HuggingFace.
//!
//! Process-wide singleton (`bootstrap()`) so concurrent transcribe
//! requests share one download instead of racing for the same file.
//! Status reads are non-blocking — a polling endpoint
//! (`GET /sync/whisper-status`) snapshots the current state for the
//! UI's progress bar.
//!
//! Resolution order is unchanged: the singleton respects
//! `WHISPER_MODEL_PATH` and a pre-placed cached file, falling through
//! to the network only when nothing is on disk. Once the file lands
//! it stays — the same checkout never re-downloads.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};

use super::model::{cache_path_for, download_url, resolve_model_path, DEFAULT_MODEL_ID};

/// Snapshot of the bootstrap state. The frontend polls
/// `/sync/whisper-status` and renders a progress bar from this.
#[derive(Debug, Clone)]
pub enum WhisperStatus {
    /// No download has been kicked off this process; either the
    /// feature has never been touched OR the file was already on
    /// disk from a previous run.
    Idle,
    /// HTTP transfer in flight. `total` is `None` until the response
    /// headers arrive and is `Some` once `Content-Length` is known.
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        model_id: String,
    },
    /// The model file exists on disk and the next transcription will
    /// load it. (We don't actively probe whisper-rs's `WhisperContext`
    /// at startup — that happens lazily inside `transcribe.rs`.)
    Ready { path: PathBuf },
    /// Download (or resolution) failed; the message is what surfaces
    /// in the toast. Caller may retry — the next `ensure_model_present`
    /// call resets to `Downloading` and tries again.
    Failed { error: String },
}

/// Process-wide bootstrap. `Mutex<()>` over the download itself serial-
/// ises racing first-use calls. `RwLock<WhisperStatus>` is the
/// non-blocking readers' side — the status endpoint never waits on a
/// download to acquire the read lock.
pub struct WhisperBootstrap {
    status: RwLock<WhisperStatus>,
    download_lock: Mutex<()>,
}

impl WhisperBootstrap {
    fn new() -> Self {
        Self {
            status: RwLock::new(WhisperStatus::Idle),
            download_lock: Mutex::new(()),
        }
    }

    pub async fn status(&self) -> WhisperStatus {
        self.status.read().await.clone()
    }

    /// Resolve the GGML file to transcribe with. Returns the path
    /// directly when something is already on disk; otherwise downloads
    /// `model_id` from HuggingFace, updating `self.status` with
    /// progress as the response body streams in.
    ///
    /// Serialised: two concurrent first-use calls collapse into one
    /// download. The second call waits on `download_lock`, then sees
    /// the freshly-cached file when it re-checks `resolve_model_path`.
    pub async fn ensure_model_present(&self, model_id: &str) -> Result<PathBuf> {
        // Fast path: file already there — no lock acquisition needed
        // beyond the status RwLock for the publish.
        if let Some(p) = resolve_model_path()? {
            *self.status.write().await = WhisperStatus::Ready { path: p.clone() };
            return Ok(p);
        }

        let _g = self.download_lock.lock().await;

        // Re-check under the lock — a concurrent caller may have
        // finished the download we were queued for.
        if let Some(p) = resolve_model_path()? {
            *self.status.write().await = WhisperStatus::Ready { path: p.clone() };
            return Ok(p);
        }

        let target = cache_path_for(model_id)?;
        let url = download_url(model_id);
        tracing::info!(
            "[whisper] no GGML model on disk — downloading {} from {}",
            model_id,
            url
        );

        *self.status.write().await = WhisperStatus::Downloading {
            downloaded: 0,
            total: None,
            model_id: model_id.to_string(),
        };

        let result = self.download_to(&url, model_id, &target).await;
        match result {
            Ok(()) => {
                tracing::info!("[whisper] model ready at {}", target.display());
                *self.status.write().await = WhisperStatus::Ready {
                    path: target.clone(),
                };
                Ok(target)
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::warn!("[whisper] download failed: {msg}");
                *self.status.write().await = WhisperStatus::Failed { error: msg };
                Err(e)
            }
        }
    }

    /// Stream the response body into `<target>.part`, then atomic-
    /// rename to `<target>` on success. Partial files are deleted on
    /// error so a retry starts clean. Uses the shared `reqwest`
    /// client (no new TLS stack pulled in).
    async fn download_to(
        &self,
        url: &str,
        model_id: &str,
        target: &std::path::Path,
    ) -> Result<()> {
        let part = target.with_extension("part");
        // Best-effort cleanup of any stale .part from a prior failed
        // attempt — we don't honour HTTP Range yet (Phase 1.7 if it
        // ever matters), so resuming is equivalent to starting over.
        let _ = tokio::fs::remove_file(&part).await;

        let client = reqwest::Client::builder()
            .user_agent("MikeRust/audio-bootstrap")
            .build()
            .context("reqwest client")?;
        let resp = client
            .get(url)
            .send()
            .await
            .context("whisper download GET")?
            .error_for_status()
            .context("whisper download non-success")?;

        let total = resp.content_length();
        if let WhisperStatus::Downloading { ref mut total: t, .. } =
            *self.status.write().await
        {
            *t = total;
        }

        let mut file = tokio::fs::File::create(&part)
            .await
            .with_context(|| format!("create {}", part.display()))?;

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        let mut downloaded: u64 = 0;
        // Throttle status writes — every chunk would lock the RwLock
        // far more often than the UI polls (defaults to ~1 Hz). One
        // write per ~1 MB of throughput is plenty.
        const STATUS_TICK: u64 = 1 << 20;
        let mut next_tick: u64 = STATUS_TICK;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("whisper download chunk")?;
            file.write_all(&chunk).await.context("part write")?;
            downloaded += chunk.len() as u64;
            if downloaded >= next_tick {
                next_tick = downloaded + STATUS_TICK;
                *self.status.write().await = WhisperStatus::Downloading {
                    downloaded,
                    total,
                    model_id: model_id.to_string(),
                };
            }
        }
        file.flush().await.context("part flush")?;
        drop(file);

        // Atomic-ish rename: tokio::fs::rename → SetFileInformationByHandle
        // on Windows, ::rename on Unix. As good as it gets without
        // also fsyncing the parent dir; for a one-time bootstrap we
        // accept that.
        tokio::fs::rename(&part, target)
            .await
            .with_context(|| format!("rename {} → {}", part.display(), target.display()))?;
        Ok(())
    }
}

/// Process-wide singleton accessor. Constructed on first call.
pub fn bootstrap() -> Arc<WhisperBootstrap> {
    static INST: OnceLock<Arc<WhisperBootstrap>> = OnceLock::new();
    INST.get_or_init(|| Arc::new(WhisperBootstrap::new())).clone()
}

/// Convenience: ensure the *default* model is on disk. Used by
/// `transcribe_audio` and by the eager warm-up call we plug into
/// server startup.
pub async fn ensure_default_model() -> Result<PathBuf> {
    let model_id = std::env::var("WHISPER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL_ID.to_string());
    bootstrap()
        .ensure_model_present(&model_id)
        .await
        .with_context(|| format!("ensure_default_model({model_id})"))
        .map_err(|e| anyhow!("{e:#}"))
}
