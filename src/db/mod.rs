use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::auth::SessionStore;

#[cfg(feature = "rag")]
use crate::embeddings::EmbeddingService;
#[cfg(feature = "rag")]
use crate::sync::scanner::ScanProgressHandle;

/// Resolve the SQLite path to a location *outside* the project tree.
///
/// Tauri dev's file watcher rebuilds whenever any file under `src-tauri/`
/// changes. SQLite in WAL mode constantly rewrites `.db-wal` and
/// `.db-shm`, so a DB anywhere under the project triggers an infinite
/// rebuild loop. Default location is `<user-home>/mikerust-data/mike.db`
/// — overridable via `DATABASE_URL` for tests / CI.
fn default_db_url() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let path = PathBuf::from(home).join("mikerust-data").join("mike.db");
    // SQLite URI on Windows requires forward slashes after `sqlite:`.
    format!("sqlite:{}", path.display().to_string().replace('\\', "/"))
}

/// Request sent through the biometric channel.
/// The Tauri side receives it, shows the OS dialog with the correct HWND,
/// and sends back Ok(true/false) or Err(message).
pub type BiometricRequest = (String, oneshot::Sender<Result<bool, String>>);

/// How long an MCP discovery snapshot stays valid before we re-run the
/// `initialize → tools/list → prompts/list` handshake. Five minutes
/// matches the typical horizon at which an MCP server might cycle a
/// session id; before this cache, every chat turn paid the full
/// handshake cost on every configured server. Configurable via env
/// override `MCP_CACHE_TTL_SECS` for tuning / tests.
pub fn mcp_cache_ttl() -> std::time::Duration {
    std::env::var("MCP_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(300))
}

/// How long to wait for an MCP `tools/call` response before giving up.
/// Default 5 minutes — pseudonymization, OCR, RAG-summary tools can
/// realistically take 60-120 s on a non-trivial doc, and the previous
/// 60 s default tripped over them: every long call returned an opaque
/// `{"error":"network: timeout"}` string and the model would tell the
/// user "communication error" instead of waiting. Override via env
/// `MCP_CALL_TIMEOUT_SECS` for shops that prefer to fail faster.
pub fn mcp_call_timeout_secs() -> u64 {
    std::env::var("MCP_CALL_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0 && n <= 1800)
        .unwrap_or(300)
}

/// Stored per-user. We carry the discovery payload as opaque JSON so
/// `db/mod.rs` doesn't depend on the routes layer's `McpDiscovered`
/// type — the chat handler serialises into this on insert and
/// deserialises on read. Cheap (a handful of MCP servers per user;
/// each one a few hundred bytes of JSON).
#[derive(Clone)]
pub struct McpDiscoveryCacheEntry {
    pub stored_at: std::time::Instant,
    /// JSON-encoded `Vec<McpDiscovered>`. Kept as a string to keep
    /// this module dependency-free.
    pub payload_json: String,
}

impl McpDiscoveryCacheEntry {
    pub fn is_fresh(&self, ttl: std::time::Duration) -> bool {
        self.stored_at.elapsed() < ttl
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub sessions: SessionStore,
    /// Send a biometric verification request to the Tauri window process.
    /// None when running outside Tauri (e.g. standalone server).
    pub biometric_tx: Option<mpsc::Sender<BiometricRequest>>,
    /// Cache of model identifiers that the upstream LLM provider has rejected
    /// for `tools=[…]` (e.g. Ollama Gemma3 returns "does not support tools").
    /// Avoids paying the round-trip on every chat request.
    pub no_tools_models: Arc<RwLock<HashSet<String>>>,

    /// Per-user MCP discovery cache. Avoids re-running the
    /// `initialize → notifications/initialized → tools/list → prompts/list`
    /// handshake on every chat turn — without this every user message
    /// hammered every configured MCP server with a fresh session id.
    /// TTL-based; entries older than `MCP_CACHE_TTL` are re-discovered
    /// on the next chat. Manually invalidated when the user updates
    /// MCP server settings (POST/PUT/DELETE on /user/mcp).
    pub mcp_discovery_cache:
        Arc<RwLock<HashMap<String, McpDiscoveryCacheEntry>>>,

    /// Process-wide embedding service (loads multilingual-e5-base once
    /// on first use and reuses it). `None` when the `rag` feature is
    /// disabled at compile time, OR when STORAGE_PATH wasn't configured
    /// (which we need as the on-disk root for per-user Lance dbs).
    #[cfg(feature = "rag")]
    pub embeddings: Option<Arc<EmbeddingService>>,

    /// In-memory map of in-flight scan progress, keyed by `sync_folders.id`.
    /// Populated when `/sync/folders/{id}/scan` kicks off a job; read by
    /// the status endpoint. Cleared when the user removes the folder.
    #[cfg(feature = "rag")]
    pub scans: Arc<RwLock<HashMap<String, ScanProgressHandle>>>,

    /// JSON-driven corpus plugin registry, loaded once at startup from
    /// `MIKE_CORPUS_PLUGINS_DIR` (or walks ancestors for `corpora-plugins`
    /// by default). Read by the `/corpora` endpoint and by the chat
    /// library-inventory builder. Empty when no manifest directory
    /// exists — the hardcoded EUR-Lex / Italian routes still work,
    /// the registry is purely metadata for discovery and UI.
    pub corpus_plugins: Arc<Vec<crate::corpora::plugin::CorpusPlugin>>,

    /// Per-corpus runtime adapter for declarative corpora
    /// (`strategy.kind == "http-fetch-per-id"`). Keyed by corpus id.
    /// Built once at startup from `corpus_plugins`. The generic
    /// `/corpora/:id/{search,fetch}` routes look up this map to
    /// dispatch HTTP fetch + extraction without per-corpus Rust code.
    ///
    /// Builtin corpora (EUR-Lex, Italian Legal) are NOT in this
    /// registry today — their existing `/eurlex/*` /
    /// `/italian-legal/*` routes call their adapters directly. They
    /// migrate here when we converge on generic routes.
    pub corpus_adapters:
        Arc<crate::corpora::manifest_adapter::AdapterRegistry>,

    /// Live progress for in-flight bulk imports, keyed by corpus id.
    /// Spawned by POST `/corpora/:id/import`, polled by GET
    /// `/corpora/:id/import-progress`. A `phase=="error"` entry
    /// sticks until the next import overwrites it so the UI can
    /// surface the message after the user looks away.
    pub corpus_import_progress: Arc<
        RwLock<HashMap<String, Arc<RwLock<crate::corpora::dila_bulk::ImportProgress>>>>,
    >,
}

impl AppState {
    pub async fn new() -> Result<Self> {
        // Register sqlite-vec as a SQLite auto-extension BEFORE we open
        // any connection. This way every connection sqlx creates
        // (including the one running migrations) gets the `vec0`
        // virtual-table module loaded — required by migration 0009 and
        // by every embedding query later on.
        //
        // The cast goes through `*const ()` because libsqlite3-sys'
        // `sqlite3_auto_extension` expects a generic init function
        // pointer, while sqlite-vec exposes a specifically-typed one.
        // Both ABIs match what SQLite calls at extension load time.
        #[cfg(feature = "rag")]
        {
            crate::embeddings::register_sqlite_vec_auto_extension();
            tracing::info!("[rag] sqlite-vec auto-extension registered");
        }

        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| default_db_url());

        // SQLite won't auto-create the parent directory; do it explicitly
        // so `<user-home>/mikerust-data/` exists on first run.
        if let Some(file_path) = db_url.strip_prefix("sqlite:") {
            // Strip query string if any (e.g. ?mode=rwc) before mkdir.
            let raw = file_path.split('?').next().unwrap_or(file_path);
            // Tolerate both `/` and `\` in the URL.
            let pb = PathBuf::from(raw.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = pb.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
        }
        tracing::info!("[db] using DATABASE_URL={db_url}");

        let opts = SqliteConnectOptions::from_str(&db_url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let db = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        let sessions = SessionStore::new(db.clone());

        // Bootstrap the RAG embedding service. The vector store lives
        // in the same SQLite file via the sqlite-vec virtual table —
        // we just hand the pool to the service. The migration adds the
        // `doc_chunks` vec0 table; if that already ran we're ready.
        #[cfg(feature = "rag")]
        let embeddings: Option<Arc<EmbeddingService>> =
            Some(Arc::new(EmbeddingService::new(db.clone())));

        // Load corpus plugin manifests from disk. Failures are
        // non-fatal: we log and continue with an empty registry — the
        // hardcoded EUR-Lex / Italian routes still serve their requests
        // regardless of what's in the registry.
        let plugins_dir = crate::corpora::plugin::plugins_dir();
        let corpus_plugins = match crate::corpora::plugin::load_plugins(&plugins_dir) {
            Ok(p) => {
                tracing::info!(
                    "[corpus-plugins] {} manifest(s) loaded from {}",
                    p.len(),
                    plugins_dir.display()
                );
                p
            }
            Err(e) => {
                tracing::warn!(
                    "[corpus-plugins] load failed from {}: {:#}",
                    plugins_dir.display(),
                    e
                );
                Vec::new()
            }
        };
        // Build the runtime adapter registry for declarative corpora.
        // Builtin corpora are intentionally NOT inserted here yet —
        // see comment on AppState::corpus_adapters.
        let corpus_adapters =
            crate::corpora::manifest_adapter::build_adapter_registry(&corpus_plugins);
        tracing::info!(
            "[corpus-adapters] {} declarative adapter(s) registered",
            corpus_adapters.len()
        );

        Ok(Self {
            db,
            sessions,
            biometric_tx: None,
            no_tools_models: Arc::new(RwLock::new(HashSet::new())),
            mcp_discovery_cache: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "rag")]
            embeddings,
            #[cfg(feature = "rag")]
            scans: Arc::new(RwLock::new(HashMap::new())),
            corpus_plugins: Arc::new(corpus_plugins),
            corpus_adapters: Arc::new(corpus_adapters),
            corpus_import_progress: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Invalidate any cached MCP discovery for this user. Called from
    /// the /user/mcp settings endpoints whenever the user adds, edits
    /// or removes a server, so the next chat re-runs discovery instead
    /// of using a stale (possibly broken) tool list.
    pub async fn invalidate_mcp_cache_for_user(&self, user_id: &str) {
        let mut g = self.mcp_discovery_cache.write().await;
        g.remove(user_id);
    }

    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.db).await?;
        self.sessions.purge_expired().await?;
        Ok(())
    }
}
