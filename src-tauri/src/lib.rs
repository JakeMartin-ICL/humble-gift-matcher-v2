mod credential_store;
mod entitlements;
mod game_details;
mod humble;
mod reviews;
mod state;
mod steam;
mod wishlists;

use state::{AppState, AppView};
use tauri::State;

#[tauri::command]
async fn get_app_view(state: State<'_, AppState>) -> Result<AppView, String> {
    Ok(state.view.read().await.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            use tauri::Manager;

            let main_window = app
                .get_webview_window("main")
                .ok_or("The main application window was not created.")?;
            main_window.center()?;
            main_window.maximize()?;

            let state = app.state::<AppState>().inner().clone();
            tauri::async_runtime::spawn(async move {
                humble::resume_saved_session(&state).await;
                if state.humble_session.read().await.is_some() {
                    let _ = entitlements::load_cached_or_refresh(state.clone()).await;
                }
                steam::resume_saved_login(state).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_view,
            steam::start_steam_login,
            steam::cancel_steam_login,
            steam::disconnect_steam,
            humble::start_humble_login,
            humble::check_humble_login,
            humble::connect_humble_with_cookie,
            humble::disconnect_humble,
            entitlements::refresh_humble_entitlements,
            entitlements::cancel_humble_refresh,
            reviews::load_steam_reviews,
            game_details::load_steam_game_details,
            game_details::open_steam_game,
            humble::open_humble_entitlement,
            wishlists::refresh_wishlists,
            wishlists::search_steam_apps,
            wishlists::set_entitlement_mapping,
            wishlists::clear_entitlement_mapping
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
