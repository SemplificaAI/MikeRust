pub mod auth;
pub mod corpora;
pub mod db;
pub mod docx;
pub mod domain;
pub mod embeddings;
pub mod http_client;
pub mod llm;
pub mod mcp;
pub mod mikeprj;
pub mod pdf;
pub mod presets;
pub mod routes;
pub mod storage;
pub mod sync;

pub use db::AppState;

use axum::{Router, extract::DefaultBodyLimit, http::Method};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Start the axum HTTP server on the given port.
/// Blocks until the server shuts down.
/// Intended to be called from a dedicated tokio task or thread.
///
/// Pass `port = 0` to let the OS pick a free high port. The actual
/// bound port is logged at startup; callers that need to discover it
/// (e.g. the Tauri shell so it can tell the frontend) should use
/// `run_server_with_bio_tx` and pass a `port_tx` oneshot.
pub use db::BiometricRequest;

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    run_server_with_channels(port, None, None).await
}

pub async fn run_server_with_bio_tx(
    port: u16,
    biometric_tx: Option<tokio::sync::mpsc::Sender<BiometricRequest>>,
) -> anyhow::Result<()> {
    run_server_with_channels(port, biometric_tx, None).await
}

/// Load `.env` from a known-good location regardless of cwd.
///
/// Tauri spawns the bundled exe with `cwd = src-tauri/`, where there's no
/// `.env`. Plain `dotenvy::dotenv()` only checks cwd, so the env vars we
/// rely on (DATABASE_URL, STORAGE_PATH, …) silently failed to load and
/// the DB ended up wherever the relative fallback resolved to. We walk
/// up from both cwd and the executable directory until we find a `.env`.
fn load_dotenv() {
    fn try_walk_up(start: std::path::PathBuf) -> bool {
        let mut current: Option<std::path::PathBuf> = Some(start);
        while let Some(dir) = current {
            let candidate = dir.join(".env");
            if candidate.is_file() {
                if dotenvy::from_path(&candidate).is_ok() {
                    tracing::info!("[env] loaded {}", candidate.display());
                    return true;
                }
            }
            current = dir.parent().map(|p| p.to_path_buf());
        }
        false
    }

    if let Ok(cwd) = std::env::current_dir() {
        if try_walk_up(cwd) {
            return;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            try_walk_up(parent.to_path_buf());
        }
    }
}

/// Pin fastembed's model cache to a stable directory **outside the
/// workspace**, otherwise the ~280MB of `.part` chunks downloaded on
/// first run land under the cwd (= `src-tauri/` for Tauri dev) and
/// trigger the file watcher repeatedly during the download.
///
/// Honours `FASTEMBED_CACHE_DIR` if the user already set it in `.env`;
/// otherwise points at `<userdata>/mikerust-data/fastembed`. Either
/// way the directory is created so fastembed doesn't fail on first
/// `try_new`.
///
/// Called from `run_server_with_bio_tx` immediately after `load_dotenv`,
/// so the override takes effect before the embedding service spins up.
fn ensure_fastembed_cache_dir() {
    if std::env::var("FASTEMBED_CACHE_DIR").is_ok() {
        return;
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(home)
        .join("mikerust-data")
        .join("fastembed");
    let _ = std::fs::create_dir_all(&path);
    // SAFETY: single-threaded process startup before the runtime spins
    // up — no concurrent reads of std::env to race with.
    unsafe {
        std::env::set_var("FASTEMBED_CACHE_DIR", &path);
    }
    tracing::info!("[rag] fastembed cache pinned to {}", path.display());
}

/// Install a panic hook that logs the panic through `tracing::error!`
/// before the default behaviour (write to stderr) fires. This is the
/// minimum viable observability: every panic ends up in the same
/// structured log channel as the rest of the backend, with the
/// thread name, payload, and source location attached. Crucially the
/// hook does NOT swallow the panic — the thread still unwinds, the
/// tokio task still aborts. Tasks that need survival semantics must
/// use `tokio::task::spawn` with `catch_unwind` or the
/// `tokio::task::JoinError::is_panic()` branch at the join site.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");
        let payload = info
            .payload()
            .downcast_ref::<&'static str>()
            .map(|s| *s)
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string payload>");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        tracing::error!(
            target: "mike::panic",
            thread = %name,
            location = %location,
            "panic in mike: {payload}"
        );
        default(info);
    }));
}

pub async fn run_server_with_channels(
    port: u16,
    biometric_tx: Option<tokio::sync::mpsc::Sender<BiometricRequest>>,
    port_tx: Option<tokio::sync::oneshot::Sender<u16>>,
) -> anyhow::Result<()> {
    install_panic_hook();
    load_dotenv();
    ensure_fastembed_cache_dir();
    // Point ort's `load-dynamic` loader at our vendored
    // `libs/onnxruntime/<platform>/` DLL before any embedding code
    // touches the runtime. Must happen pre-AppState::new (which
    // constructs EmbeddingService) so the env var is visible by the
    // time fastembed initialises its first session.
    #[cfg(feature = "rag")]
    crate::embeddings::service::ensure_onnxruntime_dylib_path();

    let mut state = AppState::new().await?;
    state.biometric_tx = biometric_tx;
    let state = Arc::new(state);
    state.run_migrations().await?;

    // Startup recovery: any document still flagged as `syncing` from a
    // previous session can't actually be in flight any more — there's
    // no embedding task running for it. Flip those rows to
    // `interrupted` so the UI surfaces the resync button instead of
    // leaving them stuck with a spinner that never moves.
    let recovered = sqlx::query(
        "UPDATE documents SET status = 'interrupted' WHERE status = 'syncing'",
    )
    .execute(&state.db)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);
    if recovered > 0 {
        tracing::info!(
            "[startup] recovered {recovered} doc(s) from stale 'syncing' state \
             → marked 'interrupted' (resync from the UI when ready)"
        );
    }

    // Restrict CORS to the origins that actually need it: the Next.js
    // dev server, the Tauri webview, and (via env var) any extra origin
    // a deployer wants to allow explicitly. Previously this was wide-open
    // (`allow_origin(Any)`), which is fine inside Tauri's `tauri://`
    // webview but lets *any* website hit the local API and exfiltrate
    // the bearer token from `localStorage` if a user ever opens the
    // backend port in a regular browser tab.
    //
    // Override at runtime with `MIKE_ALLOWED_ORIGINS=https://x,https://y`.
    let allowlist: Vec<axum::http::HeaderValue> = std::env::var("MIKE_ALLOWED_ORIGINS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            vec![
                "http://localhost:3000".to_string(),
                "http://localhost:3001".to_string(),
                "http://127.0.0.1:3000".to_string(),
                "http://127.0.0.1:3001".to_string(),
                "tauri://localhost".to_string(),
                "https://tauri.localhost".to_string(),
            ]
        })
        .into_iter()
        .filter_map(|s| s.parse::<axum::http::HeaderValue>().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowlist))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
        ])
        .allow_credentials(false);

    // Global request body limit. The default axum limit (2 MB) is too
    // small for chat history with multi-doc attachments, but unbounded
    // is a DoS vector. 50 MB is a comfortable ceiling for every route
    // that ISN'T document upload (which has its own 100 MB layer set
    // in `routes::documents::router()`).
    let global_body_limit = DefaultBodyLimit::max(50 * 1024 * 1024);

    let app = Router::new()
        .nest("/auth",     routes::auth::router())
        .nest("/user",     routes::user::router())
        .nest("/chat",     routes::chat::router())
        .nest("/project",  routes::projects::router())
        .nest("/document", routes::documents::router())
        // Alias used by the upstream-Mike frontend for standalone documents.
        .nest("/single-documents", routes::documents::router())
        .nest("/workflow",  routes::workflows::router())
        .nest("/column-presets", routes::presets::router())
        .nest("/docx-templates", routes::docx_templates::router())
        .nest("/models", routes::models::router())
        .nest("/tabular-review", routes::tabular_reviews::router())
        .nest("/sync",     routes::sync::router())
        .nest("/eurlex",   routes::eurlex::router())
        .nest("/italian-legal", routes::italian_legal::router())
        .nest("/corpora",  routes::corpora::router())
        .nest("/healthz",  routes::health::router())
        .layer(cors)
        .layer(global_body_limit)
        .with_state(state);

    // Bind: when `port == 0`, the OS picks a free high port — we then
    // read it back from the listener and report it via `port_tx` so the
    // Tauri shell can forward the actual URL to the frontend (which
    // can't know it ahead of time). When `port != 0` we honour it as
    // a fixed bind (useful for standalone backend dev where the
    // frontend uses `NEXT_PUBLIC_API_BASE_URL` to find us).
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let actual_port = listener.local_addr()?.port();
    tracing::info!("API listening on 127.0.0.1:{actual_port}");
    if let Some(tx) = port_tx {
        let _ = tx.send(actual_port);
    }
    axum::serve(listener, app).await?;
    Ok(())
}
