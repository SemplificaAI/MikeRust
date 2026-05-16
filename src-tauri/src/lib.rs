use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tokio::sync::{mpsc, oneshot};

/// State managed by Tauri: the actual port the axum server bound to.
/// Filled in once the server has started up (via the `port_tx` oneshot).
/// Read by the `api_base_url` invoke handler so the frontend can
/// discover where the backend is without a build-time constant.
///
/// Stored as `Arc<Mutex<Option<String>>>` so the same handle can be
/// cloned into the background port-watch task AND managed by Tauri
/// for invoke-handler reads, without resorting to raw pointers.
#[derive(Clone)]
struct ApiBaseUrl(Arc<Mutex<Option<String>>>);

impl ApiBaseUrl {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
    fn set(&self, url: String) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(url);
        }
    }
    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

/// Entry point called by main.rs.
/// Starts the axum server as a background tokio task, then launches Tauri.
pub fn run() {
    dotenvy::dotenv().ok();

    // Init tracing once
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "mike=debug,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .try_init();

    // Biometric channel: axum sends requests, Tauri processes them with HWND
    let (bio_tx, mut bio_rx) = mpsc::channel::<mike::BiometricRequest>(4);

    // Port discovery channel: axum reports the OS-assigned port back so
    // the Tauri shell can hand it to the frontend on demand. Default
    // mode is "OS picks a free high port" (PORT env unset). PORT can
    // still pin a specific port for the standalone-backend dev story
    // (running the frontend in a regular browser at :3000 with
    // NEXT_PUBLIC_API_BASE_URL=http://localhost:<port>).
    let (port_tx, port_rx) = oneshot::channel::<u16>();
    let api_base = ApiBaseUrl::new();
    let api_base_for_task = api_base.clone();

    // Spawn the axum server on a background tokio runtime
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let port: u16 = std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0); // 0 = let the OS pick a free high port
            if let Err(e) = mike::run_server_with_channels(
                port,
                Some(bio_tx),
                Some(port_tx),
            ).await {
                tracing::error!("axum server error: {e}");
            }
        });
    });

    // Background task: wait for the axum server to report its bound
    // port, then stash it in the shared `ApiBaseUrl` handle so the
    // `api_base_url` invoke handler can read it. Done out of Tauri's
    // `setup` so the main thread isn't blocked — the IPC roundtrip
    // from the frontend's first `invoke("api_base_url")` races against
    // bind, but axum bind is sub-millisecond so the race resolves
    // before the user can trigger any user-driven fetch.
    tauri::async_runtime::spawn(async move {
        if let Ok(port) = port_rx.await {
            let url = format!("http://127.0.0.1:{port}");
            tracing::info!("[tauri] api_base_url resolved to {url}");
            api_base_for_task.set(url);
        } else {
            tracing::warn!(
                "[tauri] axum server never reported a port — \
                 api_base_url will stay None and the frontend will \
                 fall back to NEXT_PUBLIC_API_BASE_URL"
            );
        }
    });

    tauri::Builder::default()
        .manage(api_base)
        .invoke_handler(tauri::generate_handler![
            open_external_url,
            api_base_url,
            pick_folder
        ])
        .setup(move |app| {

            #[cfg(debug_assertions)]
            app.get_webview_window("main")
                .expect("main window")
                .open_devtools();

            // Spawn task that handles biometric requests using the Tauri window HWND
            let window = app.get_webview_window("main").expect("main window");
            tauri::async_runtime::spawn(async move {
                tracing::info!("[tauri-bio] biometric channel listener started");
                while let Some((reason, reply)) = bio_rx.recv().await {
                    tracing::info!("[tauri-bio] received request: '{reason}'");
                    let result = verify_with_window(&window, &reason);
                    tracing::info!("[tauri-bio] verify_with_window result: {:?}", result);
                    let _ = reply.send(result);
                }
                tracing::warn!("[tauri-bio] biometric channel closed");
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Call Windows Hello from the Tauri task context.
///
/// Uses the COM interop API `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`
/// so the OS dialog is parented to our Tauri window — without this, the dialog
/// can appear behind the app window and the user may miss it.
#[cfg(target_os = "windows")]
fn verify_with_window(
    window: &tauri::WebviewWindow,
    reason: &str,
) -> Result<bool, String> {
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier,
        UserConsentVerifierAvailability,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::HSTRING;
    use windows_future::IAsyncOperation;

    tracing::info!("[tauri-bio] verify_with_window: checking availability");
    let avail = UserConsentVerifier::CheckAvailabilityAsync()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] CheckAvailabilityAsync error: {e}"); e.to_string() })?
        .get()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] availability .get() error: {e}"); e.to_string() })?;

    tracing::info!("[tauri-bio] availability value: {}", avail.0);
    if !matches!(avail, UserConsentVerifierAvailability::Available) {
        return Err(format!("Windows Hello not available (code {})", avail.0));
    }

    // Bring our Tauri window to the foreground so the OS-level dialog inherits
    // focus from a visible parent. Best-effort — failure isn't fatal.
    let _ = window.set_focus();
    let _ = window.unminimize();

    let raw_hwnd = window
        .hwnd()
        .map_err(|e| { tracing::error!("[tauri-bio] window.hwnd() error: {e}"); format!("hwnd error: {e}") })?;
    let hwnd = HWND(raw_hwnd.0 as *mut core::ffi::c_void);
    tracing::info!("[tauri-bio] obtained HWND: {:?}", hwnd.0);

    let interop: IUserConsentVerifierInterop =
        windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .map_err(|e: windows::core::Error| {
                tracing::error!("[tauri-bio] interop factory error: {e}");
                format!("interop factory error: {e}")
            })?;

    let message = HSTRING::from(reason);
    tracing::info!("[tauri-bio] calling RequestVerificationForWindowAsync('{reason}')");
    let op: IAsyncOperation<UserConsentVerificationResult> = unsafe {
        interop
            .RequestVerificationForWindowAsync(hwnd, &message)
            .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] interop call error: {e}"); e.to_string() })?
    };
    let result: UserConsentVerificationResult = op
        .get()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] .get() error: {e}"); e.to_string() })?;

    tracing::info!("[tauri-bio] verification result code: {}", result.0);
    Ok(matches!(result, UserConsentVerificationResult::Verified))
}

#[cfg(not(target_os = "windows"))]
fn verify_with_window(
    _window: &tauri::WebviewWindow,
    _reason: &str,
) -> Result<bool, String> {
    Err("Biometric not supported on this platform".into())
}

/// Open a URL in the system default browser.
///
/// Tauri's WebView intercepts plain `<a target="_blank">` clicks and
/// opens them inside the same WebView, which makes the in-app shell
/// behave like a mini-browser. Routing through the OS launcher (via
/// the `open` crate) hands the URL to whatever the user's default
/// browser is, which is what they expect when clicking "Apri su
/// EUR-Lex".
///
/// Validates the scheme — only `http://` and `https://` URLs are
/// accepted, so a malicious payload from a tool result can't
/// trigger a `file://` or `mailto:` action through this command.
/// Return the actual base URL of the embedded axum HTTP server.
///
/// The shell launches axum with `port = 0` so the OS picks a free
/// high port; we then store the resulting `http://127.0.0.1:<port>`
/// here. The frontend calls this once at boot to discover where to
/// `fetch()`. Returns an empty string before the server has reported
/// its port (so the frontend can fall back to NEXT_PUBLIC_API_BASE_URL).
#[tauri::command]
fn api_base_url(state: State<'_, ApiBaseUrl>) -> String {
    state.get().unwrap_or_default()
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(format!("rejected non-http(s) URL: {url}"));
    }
    open::that(&url).map_err(|e| e.to_string())
}

/// Open the native folder picker. Returns the selected absolute path,
/// or `None` if the user cancelled. Used by the Settings sync-folder
/// field so a path can be chosen instead of typed.
#[tauri::command]
async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().into_owned())
}
