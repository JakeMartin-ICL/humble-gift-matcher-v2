use crate::credential_store;
use crate::entitlements::{self, HumbleSessionValidationError};
use crate::state::AppState;
use tauri::{
    AppHandle, Manager, State, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    webview::PageLoadEvent,
};

const LOGIN_WINDOW_LABEL: &str = "humble-login";
const BROWSER_WINDOW_LABEL: &str = "humble-browser";
const HUMBLE_LOGIN_URL: &str = "https://www.humblebundle.com/login";
const BROWSER_BOOTSTRAP_URL: &str = "https://www.humblebundle.com/robots.txt";
const SESSION_COOKIE: &str = "_simpleauth_sess";

#[tauri::command]
pub async fn start_humble_login(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        state
            .update_view(|view| {
                view.humble.phase = "waiting".to_string();
                view.humble.message =
                    "Complete sign-in in the dedicated Humble window.".to_string();
                view.humble.error = None;
            })
            .await;
        return Ok(());
    }

    let url = Url::parse(HUMBLE_LOGIN_URL).map_err(|error| error.to_string())?;
    *state.humble_rejected_session_signature.lock().await = None;
    let page_load_state = state.inner().clone();
    WebviewWindowBuilder::new(&app, LOGIN_WINDOW_LABEL, WebviewUrl::External(url))
        .title("Sign in to Humble Bundle")
        .inner_size(920.0, 760.0)
        .min_inner_size(720.0, 560.0)
        .center()
        .incognito(true)
        .on_navigation(is_allowed_humble_navigation)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let state = page_load_state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = capture_humble_session(&window, &state).await
                    && state.humble_session.read().await.is_none()
                {
                    state
                        .update_view(|view| {
                            view.humble.phase = "error".to_string();
                            view.humble.error = Some(error);
                        })
                        .await;
                }
            });
        })
        .build()
        .map_err(|error| error.to_string())?;

    state
        .update_view(|view| {
            view.humble.phase = "waiting".to_string();
            view.humble.message = "Complete sign-in in the dedicated Humble window.".to_string();
            view.humble.error = None;
        })
        .await;
    Ok(())
}

#[tauri::command]
pub async fn check_humble_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) else {
        if state.view.read().await.humble.phase == "waiting" {
            state
                .update_view(|view| {
                    view.humble.phase = "disconnected".to_string();
                    view.humble.message =
                        "The Humble sign-in window was closed before login completed.".to_string();
                })
                .await;
        }
        return Ok(false);
    };

    capture_humble_session(&window, state.inner()).await
}

#[tauri::command]
pub async fn connect_humble_with_cookie(
    state: State<'_, AppState>,
    session: String,
) -> Result<(), String> {
    if !credential_store::development_cache_enabled() {
        return Err(
            "Manual Humble session entry requires the insecure development cache opt-in."
                .to_string(),
        );
    }

    let session = session.trim();
    if session.is_empty() || session.len() < 16 || session.len() > 16_384 {
        return Err("Enter a valid Humble session value".to_string());
    }
    let state = state.inner();
    state
        .update_view(|view| {
            view.humble.phase = "connecting".to_string();
            view.humble.message = "Validating the Humble session…".to_string();
            view.humble.error = None;
        })
        .await;
    match entitlements::validate_humble_session(session).await {
        Ok(()) => {
            remember_humble_session(state, session.to_string()).await?;
            entitlements::start_refresh(state.clone()).await
        }
        Err(HumbleSessionValidationError::Expired) => {
            forget_expired_session(state).await?;
            Err("The Humble session expired. Sign in again.".to_string())
        }
        Err(HumbleSessionValidationError::Unavailable(error)) => {
            state
                .update_view(|view| {
                    view.humble.phase = "error".to_string();
                    view.humble.message =
                        "Humble could not verify the session just now.".to_string();
                    view.humble.error = Some(error.clone());
                })
                .await;
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn disconnect_humble(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = window.close();
    }
    if let Some(window) = app.get_webview_window(BROWSER_WINDOW_LABEL) {
        let _ = window.close();
    }
    *state.humble_session.write().await = None;
    *state.humble_rejected_session_signature.lock().await = None;
    if let Some(task) = state.humble_task.lock().await.take() {
        task.abort();
    }
    credential_store::delete_humble().await?;
    entitlements::delete_cache().await?;
    state
        .update_view(|view| {
            view.humble.phase = "disconnected".to_string();
            view.humble.message =
                "Sign in on Humble's own page to load available entitlements.".to_string();
            view.humble.error = None;
            view.humble.remembered = false;
            view.entitlements = Default::default();
        })
        .await;
    Ok(())
}

#[tauri::command]
pub async fn open_humble_entitlement(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    let url = Url::parse(&url).map_err(|_| "The Humble link is invalid.".to_string())?;
    if !is_allowed_humble_navigation(&url) {
        return Err("Only secure Humble Bundle pages can be opened.".to_string());
    }
    let session = state
        .humble_session
        .read()
        .await
        .clone()
        .ok_or_else(|| "Reconnect Humble before opening this entitlement.".to_string())?;

    if let Some(window) = app.get_webview_window(BROWSER_WINDOW_LABEL) {
        window
            .set_cookie(humble_session_cookie(&session))
            .map_err(|error| error.to_string())?;
        window.navigate(url).map_err(|error| error.to_string())?;
        if window.is_visible().map_err(|error| error.to_string())? {
            window.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    // Start on an inert page so the authenticated destination cannot race the
    // session-cookie injection. Loading the destination in the builder first
    // allowed WKWebView to render Humble's signed-out response before
    // `set_cookie` completed.
    let bootstrap_url = Url::parse(BROWSER_BOOTSTRAP_URL).map_err(|error| error.to_string())?;
    let (bootstrap_loaded_tx, bootstrap_loaded_rx) = tokio::sync::oneshot::channel();
    let bootstrap_loaded_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(bootstrap_loaded_tx)));
    let page_load_tx = bootstrap_loaded_tx.clone();
    let (destination_shown_tx, destination_shown_rx) = tokio::sync::oneshot::channel();
    let page_destination_tx =
        std::sync::Arc::new(std::sync::Mutex::new(Some(destination_shown_tx)));
    let destination_shown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let page_destination_shown = destination_shown.clone();
    let window = WebviewWindowBuilder::new(
        &app,
        BROWSER_WINDOW_LABEL,
        WebviewUrl::External(bootstrap_url),
    )
    .title("Humble Bundle")
    .inner_size(1120.0, 800.0)
    .min_inner_size(760.0, 560.0)
    .center()
    .visible(false)
    .incognito(true)
    .on_navigation(is_allowed_humble_navigation)
    .on_page_load(move |window, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        if payload.url().as_str() == BROWSER_BOOTSTRAP_URL {
            if let Ok(mut sender) = page_load_tx.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        } else if !page_destination_shown.swap(true, std::sync::atomic::Ordering::AcqRel) {
            let _ = window.show();
            let _ = window.set_focus();
            if let Ok(mut sender) = page_destination_tx.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        }
    })
    .build()
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(10), bootstrap_loaded_rx)
        .await
        .map_err(|_| "The authenticated Humble window did not start in time.".to_string())?
        .map_err(|_| "The authenticated Humble window stopped unexpectedly.".to_string())?;
    if let Err(error) = install_humble_browser_session(&window, &session).await {
        let _ = window.close();
        return Err(error);
    }
    window.navigate(url).map_err(|error| error.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(45), destination_shown_rx)
        .await
        .map_err(|_| "The authenticated Humble page took too long to open.".to_string())?
        .map_err(|_| "The authenticated Humble window stopped unexpectedly.".to_string())?;
    Ok(())
}

pub async fn resume_saved_session(state: &AppState, session: Option<String>) {
    if let Some(session) = session {
        state
            .update_view(|view| {
                view.humble.phase = "connecting".to_string();
                view.humble.message = "Validating the saved Humble session…".to_string();
                view.humble.error = None;
                view.humble.remembered = true;
            })
            .await;
        match entitlements::validate_humble_session(&session).await {
            Ok(()) => {
                *state.humble_session.write().await = Some(session);
                state
                    .update_view(|view| {
                        view.humble.phase = "connected".to_string();
                        view.humble.message =
                            "Humble is connected with your saved session.".to_string();
                        view.humble.error = None;
                        view.humble.remembered = true;
                    })
                    .await;
            }
            Err(HumbleSessionValidationError::Expired) => {
                if let Err(error) = forget_expired_session(state).await {
                    state
                        .update_view(|view| {
                            view.humble.phase = "error".to_string();
                            view.humble.error = Some(error);
                        })
                        .await;
                }
            }
            Err(HumbleSessionValidationError::Unavailable(error)) => {
                state
                    .update_view(|view| {
                        view.humble.phase = "error".to_string();
                        view.humble.message =
                            "The saved Humble session could not be verified.".to_string();
                        view.humble.error = Some(error);
                        view.humble.remembered = true;
                    })
                    .await;
            }
        }
    }
}

async fn remember_humble_session(state: &AppState, session: String) -> Result<(), String> {
    credential_store::save_humble(session.clone()).await?;
    *state.humble_session.write().await = Some(session);
    *state.humble_rejected_session_signature.lock().await = None;
    state
        .update_view(|view| {
            view.humble.phase = "connected".to_string();
            view.humble.message = "Humble library connected.".to_string();
            view.humble.error = None;
            view.humble.remembered = true;
        })
        .await;
    Ok(())
}

async fn capture_humble_session(window: &WebviewWindow, state: &AppState) -> Result<bool, String> {
    let _validation_guard = state.humble_validation_gate.lock().await;

    // Another page-load callback or frontend poll may have completed login and
    // closed this WebView while this check waited for the gate.
    if state.humble_session.read().await.is_some() {
        let _ = window.close();
        return Ok(true);
    }

    // `cookies_for_url` proved unreliable with WKWebView's non-persistent data
    // store on macOS. This is a dedicated incognito WebView that can navigate
    // only to Humble domains, so inspect its complete isolated cookie store and
    // select the one session cookie we need.
    let cookies = match window.cookies() {
        Ok(cookies) => cookies,
        Err(error) if is_transient_cookie_read_error(&error.to_string()) => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Could not inspect the Humble login session: {error}"
            ));
        }
    };

    #[cfg(debug_assertions)]
    record_cookie_diagnostic(state, &cookies).await;

    let session = cookies
        .into_iter()
        .find(is_humble_session_cookie)
        .map(|cookie| cookie.value().to_string());
    let Some(session) = session else {
        return Ok(false);
    };
    let session_signature = session_signature(&session);

    if state.humble_session.read().await.as_ref() == Some(&session) {
        let _ = window.close();
        return Ok(true);
    }
    if *state.humble_rejected_session_signature.lock().await == Some(session_signature) {
        return Ok(false);
    }

    state
        .update_view(|view| {
            view.humble.phase = "connecting".to_string();
            view.humble.message = "Validating the Humble session…".to_string();
            view.humble.error = None;
        })
        .await;

    match entitlements::validate_humble_session(&session).await {
        Ok(()) => {
            remember_humble_session(state, session).await?;
            entitlements::start_refresh(state.clone()).await?;
            let _ = window.close();
            Ok(true)
        }
        Err(HumbleSessionValidationError::Expired) => {
            *state.humble_rejected_session_signature.lock().await = Some(session_signature);
            state
                .update_view(|view| {
                    view.humble.phase = "waiting".to_string();
                    view.humble.message =
                        "Complete sign-in in the dedicated Humble window.".to_string();
                    view.humble.error = None;
                })
                .await;
            Ok(false)
        }
        Err(HumbleSessionValidationError::Unavailable(error)) => {
            state
                .update_view(|view| {
                    view.humble.phase = "error".to_string();
                    view.humble.message =
                        "Humble could not verify the session just now.".to_string();
                    view.humble.error = Some(error.clone());
                })
                .await;
            Err(error)
        }
    }
}

async fn forget_expired_session(state: &AppState) -> Result<(), String> {
    *state.humble_session.write().await = None;
    state
        .update_view(|view| {
            view.humble.phase = "disconnected".to_string();
            view.humble.message = "The saved Humble session expired. Sign in again.".to_string();
            view.humble.error = None;
            view.humble.remembered = false;
            view.entitlements = Default::default();
        })
        .await;
    credential_store::forget_expired_humble().await
}

fn session_signature(session: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in session.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn is_transient_cookie_read_error(error: &str) -> bool {
    error.contains("failed to receive message from webview")
}

fn is_allowed_humble_navigation(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host == "humblebundle.com" || host.ends_with(".humblebundle.com"))
}

fn humble_session_cookie(session: &str) -> tauri::webview::Cookie<'static> {
    tauri::webview::Cookie::build((SESSION_COOKIE, session.to_string()))
        .domain(".humblebundle.com")
        .path("/")
        .secure(true)
        .http_only(true)
        .build()
}

async fn install_humble_browser_session(
    window: &WebviewWindow,
    session: &str,
) -> Result<(), String> {
    // Tauri's cookie setter dispatches work to the WebView event loop and can
    // return before WKWebView has made the value observable. Establish the
    // cookie synchronously from an inert same-origin document first.
    let cookie_value = format!(
        "{SESSION_COOKIE}={session}; Domain=.humblebundle.com; Path=/; Secure; SameSite=None"
    );
    let encoded_cookie = serde_json::to_string(&cookie_value)
        .map_err(|_| "Could not prepare the Humble browser session.".to_string())?;
    window
        .eval(format!("document.cookie = {encoded_cookie};"))
        .map_err(|error| error.to_string())?;

    let mut installed = false;
    for _ in 0..20 {
        installed = window
            .cookies()
            .map_err(|error| error.to_string())?
            .iter()
            .any(|cookie| cookie.name() == SESSION_COOKIE && cookie.value() == session);
        if installed {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    if !installed {
        return Err("The Humble browser could not install the saved login session.".to_string());
    }

    // Replace the short-lived JavaScript cookie with the protected form before
    // any account page is loaded.
    window
        .set_cookie(humble_session_cookie(session))
        .map_err(|error| error.to_string())?;
    let protected = window
        .cookies()
        .map_err(|error| error.to_string())?
        .iter()
        .any(|cookie| {
            cookie.name() == SESSION_COOKIE
                && cookie.value() == session
                && cookie.http_only() == Some(true)
        });
    if !protected {
        return Err("The Humble browser could not protect the saved login session.".to_string());
    }

    #[cfg(debug_assertions)]
    eprintln!("HUMBLE_BROWSER_SESSION=installed");
    Ok(())
}

fn is_humble_session_cookie(cookie: &tauri::webview::Cookie<'_>) -> bool {
    cookie.name() == SESSION_COOKIE
        && !cookie.value().is_empty()
        && cookie.domain().is_none_or(|domain| {
            let domain = domain.trim_start_matches('.');
            domain == "humblebundle.com" || domain.ends_with(".humblebundle.com")
        })
}

#[cfg(debug_assertions)]
async fn record_cookie_diagnostic(state: &AppState, cookies: &[tauri::webview::Cookie<'static>]) {
    let mut summary = cookies
        .iter()
        .map(|cookie| {
            format!(
                "{}@{}{}",
                cookie.name(),
                cookie.domain().unwrap_or("host-only"),
                cookie.path().unwrap_or("/")
            )
        })
        .collect::<Vec<_>>();
    summary.sort();
    let signature = summary.join(",");
    let mut previous = state.humble_cookie_signature.lock().await;
    if previous.as_deref() != Some(&signature) {
        eprintln!("HUMBLE_COOKIE_NAMES={summary:?}");
        *previous = Some(signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_limited_to_https_humble_pages() {
        assert!(is_allowed_humble_navigation(
            &Url::parse("https://www.humblebundle.com/login").unwrap()
        ));
        assert!(is_allowed_humble_navigation(
            &Url::parse("https://support.humblebundle.com/hc").unwrap()
        ));
        assert!(!is_allowed_humble_navigation(
            &Url::parse("http://www.humblebundle.com/login").unwrap()
        ));
        assert!(!is_allowed_humble_navigation(
            &Url::parse("https://humblebundle.com.evil.example/login").unwrap()
        ));
    }

    #[test]
    fn entitlement_browser_bootstrap_and_destinations_stay_on_humble() {
        assert!(is_allowed_humble_navigation(
            &Url::parse(BROWSER_BOOTSTRAP_URL).unwrap()
        ));
        assert!(is_allowed_humble_navigation(
            &Url::parse("https://www.humblebundle.com/downloads?key=safe").unwrap()
        ));
        assert!(!is_allowed_humble_navigation(
            &Url::parse("https://example.com").unwrap()
        ));
    }

    #[test]
    fn session_cookie_must_belong_to_humble() {
        let valid = tauri::webview::Cookie::build((SESSION_COOKIE, "session"))
            .domain(".humblebundle.com")
            .path("/")
            .build();
        let malicious = tauri::webview::Cookie::build((SESSION_COOKIE, "session"))
            .domain("humblebundle.com.evil.example")
            .path("/")
            .build();
        assert!(is_humble_session_cookie(&valid));
        assert!(!is_humble_session_cookie(&malicious));
    }

    #[test]
    fn changed_sessions_have_different_signatures() {
        assert_eq!(
            session_signature("guest-session"),
            session_signature("guest-session")
        );
        assert_ne!(
            session_signature("guest-session"),
            session_signature("signed-in-session")
        );
    }

    #[test]
    fn closed_webview_cookie_errors_are_retryable() {
        assert!(is_transient_cookie_read_error(
            "runtime error: failed to receive message from webview"
        ));
        assert!(!is_transient_cookie_read_error(
            "permission denied while reading cookies"
        ));
    }
}
