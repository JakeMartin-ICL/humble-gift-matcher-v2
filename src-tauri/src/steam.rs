use crate::credential_store::{self, SavedSteamLogin};
use crate::state::{AppState, SteamProfile, SteamSession};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use prost::Message;
use qrcode::QrCode;
use qrcode::render::svg;
use serde_json::Value;
use steamroom::generated::{
    CMsgClientFriendsList, CPlayerGetOwnedGamesRequest, CPlayerGetOwnedGamesResponse,
    CPlayerGetPerFriendPreferencesRequest, CPlayerGetPerFriendPreferencesResponse,
    CPlayerGetPlayerLinkDetailsRequest, CPlayerGetPlayerLinkDetailsResponse,
};
use steamroom::messages::{EMsg, header::PacketHeader};
use steamroom_client::login::LoginBuilder;
use tauri::State;

const ACCESS_TOKEN_URL: &str =
    "https://api.steampowered.com/IAuthenticationService/GenerateAccessTokenForApp/v1/";
const FRIEND_LIST_URL: &str = "https://api.steampowered.com/ISteamUserOAuth/GetFriendList/v1/";

#[tauri::command]
pub async fn start_steam_login(state: State<'_, AppState>) -> Result<(), String> {
    let saved = credential_store::load().await?.steam;
    start_login(state.inner().clone(), true, saved).await
}

#[tauri::command]
pub async fn cancel_steam_login(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(task) = state.steam_task.lock().await.take() {
        task.abort();
    }
    state
        .update_view(|view| {
            view.steam.phase = "disconnected".to_string();
            view.steam.message =
                "Connect Steam to read your identity and accessible wishlists.".to_string();
            view.steam.error = None;
            view.steam.qr_image = None;
        })
        .await;
    Ok(())
}

#[tauri::command]
pub async fn disconnect_steam(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(task) = state.steam_task.lock().await.take() {
        task.abort();
    }
    if let Some(task) = state.wishlist_task.lock().await.take() {
        task.abort();
    }
    if let Some(task) = state.steam_review_task.lock().await.take() {
        task.abort();
    }
    *state.steam_session.write().await = None;
    state.wishlist_memberships.write().await.clear();
    credential_store::delete_steam().await?;
    state
        .update_view(|view| {
            view.steam = Default::default();
            view.steam.phase = "disconnected".to_string();
            view.steam.message =
                "Connect Steam to read your identity and accessible wishlists.".to_string();
            view.wishlists = Default::default();
            view.steam_reviews = Default::default();
        })
        .await;
    Ok(())
}

pub async fn resume_saved_login(state: AppState, saved: Option<SavedSteamLogin>) {
    if saved.is_some()
        && let Err(error) = start_login(state.clone(), false, saved).await
    {
        state
            .update_view(|view| {
                view.steam.phase = "error".to_string();
                view.steam.error = Some(sanitise_error(&error));
            })
            .await;
    }
}

async fn start_login(
    state: AppState,
    allow_qr: bool,
    saved: Option<SavedSteamLogin>,
) -> Result<(), String> {
    let mut task_slot = state.steam_task.lock().await;
    if let Some(task) = task_slot.take() {
        task.abort();
    }
    *state.steam_session.write().await = None;
    state
        .update_view(|view| {
            view.steam.phase = "connecting".to_string();
            view.steam.message = "Connecting securely to Steam…".to_string();
            view.steam.error = None;
            view.steam.qr_image = None;
            view.steam.profile = None;
            view.steam.ownership_loaded = false;
            view.steam.owned_app_ids.clear();
        })
        .await;

    let task_state = state.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(error) = run_login(&task_state, allow_qr, saved).await {
            #[cfg(debug_assertions)]
            eprintln!("STEAM_AUTH_ERROR={}", sanitise_error(&error));
            task_state
                .update_view(|view| {
                    view.steam.phase = "error".to_string();
                    view.steam.message = "Steam sign-in stopped.".to_string();
                    view.steam.qr_image = None;
                    view.steam.error = Some(sanitise_error(&error));
                })
                .await;
        }
    });
    *task_slot = Some(task);
    Ok(())
}

async fn run_login(
    state: &AppState,
    allow_qr: bool,
    saved: Option<SavedSteamLogin>,
) -> Result<(), String> {
    if let Some(saved) = saved {
        state
            .update_view(|view| {
                view.steam.message = "Reconnecting with your saved Steam session…".to_string();
                view.steam.remembered = true;
            })
            .await;

        match login_with_refresh_token(&saved).await {
            Ok((client, steam_id, access_token)) => {
                return finish_login(state, client, steam_id, access_token, true).await;
            }
            Err(_) => {
                credential_store::delete_steam().await?;
                state
                    .update_view(|view| {
                        view.steam.remembered = false;
                        view.steam.message =
                            "The saved session expired. A new approval is required.".to_string();
                    })
                    .await;
                if !allow_qr {
                    return Err("The saved Steam session expired. Connect Steam again.".to_string());
                }
            }
        }
    }

    if !allow_qr {
        return Ok(());
    }

    let flow = LoginBuilder::new()
        .device_name("Humble Gift Matcher")
        .prefer_protocol(steamroom::connection::Protocol::WebSocket)
        .allow_protocol_fallback(false)
        .with_qr()
        .begin()
        .await
        .map_err(|error| error.to_string())?;

    let qr_image = qr_data_url(flow.challenge_url())?;
    state
        .update_view(|view| {
            view.steam.phase = "qr_ready".to_string();
            view.steam.message = "Scan with Steam Mobile, then approve this device.".to_string();
            view.steam.qr_image = Some(qr_image);
        })
        .await;

    let approved = flow
        .wait_for_scan()
        .await
        .map_err(|error| error.to_string())?;
    let steam_id = steam_id_from_token(&approved.tokens().access_token)?;
    let access_token = approved.tokens().access_token.clone();
    let saved_login = SavedSteamLogin {
        account_name: approved
            .tokens()
            .account_name
            .clone()
            .ok_or_else(|| "Steam did not return an account name".to_string())?,
        refresh_token: approved.tokens().refresh_token.clone(),
        access_token: Some(access_token.clone()),
    };

    state
        .update_view(|view| {
            view.steam.phase = "connecting".to_string();
            view.steam.message = "Approved. Establishing the Steam session…".to_string();
            view.steam.qr_image = None;
        })
        .await;

    let client = approved.finish().await.map_err(|error| error.to_string())?;
    credential_store::save_steam(saved_login).await?;
    finish_login(state, client, steam_id, Some(access_token), true).await
}

async fn login_with_refresh_token(
    saved: &SavedSteamLogin,
) -> Result<
    (
        steamroom::client::SteamClient<steamroom::client::LoggedIn>,
        u64,
        Option<String>,
    ),
    String,
> {
    let steam_id = steam_id_from_token(&saved.refresh_token)?;
    let access_token = if saved.access_token.is_some() {
        saved.access_token.clone()
    } else {
        #[cfg(debug_assertions)]
        {
            match generate_access_token(&saved.refresh_token, steam_id).await {
                Ok(access_token) => Some(access_token),
                Err(error) => {
                    eprintln!("STEAM_ACCESS_TOKEN_REFRESH_ERROR={error}");
                    None
                }
            }
        }
        #[cfg(not(debug_assertions))]
        {
            generate_access_token(&saved.refresh_token, steam_id)
                .await
                .ok()
        }
    };
    let client = LoginBuilder::new()
        .device_name("Humble Gift Matcher")
        .prefer_protocol(steamroom::connection::Protocol::WebSocket)
        .allow_protocol_fallback(false)
        .with_refresh_token(saved.account_name.clone(), saved.refresh_token.clone())
        .login()
        .await
        .map_err(|error| error.to_string())?;
    Ok((client, steam_id, access_token))
}

async fn finish_login(
    state: &AppState,
    client: steamroom::client::SteamClient<steamroom::client::LoggedIn>,
    steam_id: u64,
    access_token: Option<String>,
    remembered: bool,
) -> Result<(), String> {
    state
        .update_view(|view| {
            view.steam.message = "Loading your Steam identity and friends…".to_string();
        })
        .await;
    let mut friend_ids = receive_friend_ids(&client).await.unwrap_or_default();
    if let Ok(preference_friend_ids) = get_preference_friend_ids(&client, steam_id).await {
        friend_ids.extend(preference_friend_ids);
    }
    if let Some(access_token) = access_token {
        match get_oauth_friend_ids(&access_token).await {
            Ok(oauth_friend_ids) => friend_ids.extend(oauth_friend_ids),
            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!("STEAM_FRIEND_LIST_ERROR={_error}");
            }
        }
    }
    friend_ids.sort_unstable();
    friend_ids.dedup();
    #[cfg(debug_assertions)]
    eprintln!("STEAM_FRIEND_COUNT={}", friend_ids.len());
    let mut requested_ids = vec![steam_id];
    requested_ids.extend(friend_ids);
    requested_ids.sort_unstable();
    requested_ids.dedup();
    let mut profiles = get_player_profiles(&client, &requested_ids)
        .await
        .unwrap_or_default();
    let profile = profiles
        .iter()
        .find(|profile| profile.steam_id == steam_id.to_string())
        .cloned()
        .unwrap_or(SteamProfile {
            steam_id: steam_id.to_string(),
            display_name: "Steam account".to_string(),
            avatar_url: None,
        });
    profiles.retain(|candidate| candidate.steam_id != steam_id.to_string());
    state
        .update_view(|view| {
            view.steam.message = "Loading the games in your Steam library…".to_string();
        })
        .await;
    let (ownership_loaded, owned_app_ids) = match get_owned_app_ids(&client, steam_id).await {
        Ok(app_ids) => {
            #[cfg(debug_assertions)]
            eprintln!("STEAM_OWNED_GAME_COUNT={}", app_ids.len());
            (true, app_ids)
        }
        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!("STEAM_OWNED_GAMES_ERROR={}", sanitise_error(&_error));
            (false, Default::default())
        }
    };
    *state.steam_session.write().await = Some(SteamSession {
        client,
        steam_id,
        friends: profiles,
    });
    state
        .update_view(|view| {
            view.steam.phase = "connected".to_string();
            view.steam.message = "Steam is connected.".to_string();
            view.steam.error = None;
            view.steam.qr_image = None;
            view.steam.profile = Some(profile);
            view.steam.remembered = remembered;
            view.steam.ownership_loaded = ownership_loaded;
            view.steam.owned_app_ids = owned_app_ids;
        })
        .await;
    crate::wishlists::start_refresh(state.clone()).await?;
    Ok(())
}

async fn get_owned_app_ids(
    client: &steamroom::client::SteamClient<steamroom::client::LoggedIn>,
    steam_id: u64,
) -> Result<std::collections::BTreeSet<u32>, String> {
    let request = CPlayerGetOwnedGamesRequest {
        steamid: Some(steam_id),
        include_appinfo: Some(false),
        include_played_free_games: Some(true),
        include_free_sub: Some(true),
        ..Default::default()
    };
    let response = client
        .call_service_method("Player.GetOwnedGames#1", &request.encode_to_vec())
        .await
        .map_err(|error| error.to_string())?;
    let response: CPlayerGetOwnedGamesResponse =
        response.decode().map_err(|error| error.to_string())?;
    Ok(normalise_owned_app_ids(
        response.games.into_iter().map(|game| game.appid),
    ))
}

fn normalise_owned_app_ids(
    app_ids: impl IntoIterator<Item = Option<i32>>,
) -> std::collections::BTreeSet<u32> {
    app_ids
        .into_iter()
        .flatten()
        .filter_map(|app_id| u32::try_from(app_id).ok())
        .filter(|app_id| *app_id > 0)
        .collect()
}

async fn receive_friend_ids(
    client: &steamroom::client::SteamClient<steamroom::client::LoggedIn>,
) -> Result<Vec<u64>, String> {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let incoming = client.recv_msg().await.map_err(|error| error.to_string())?;
            if let Some(friend_ids) = decode_friend_ids(incoming.emsg, &incoming.body)? {
                return Ok(friend_ids);
            }
            if incoming.emsg == EMsg::MULTI {
                for message in steamroom::client::multi::unpack_multi(&incoming.body)
                    .map_err(|error| error.to_string())?
                {
                    let parsed =
                        PacketHeader::parse(&message).map_err(|error| error.to_string())?;
                    let (emsg, body) = match parsed {
                        PacketHeader::Protobuf { header, body } => (header.emsg, body),
                        PacketHeader::Simple { header, body } => (header.emsg, body),
                        PacketHeader::Extended { header, body } => (header.emsg, body),
                    };
                    if let Some(friend_ids) = decode_friend_ids(emsg, &body)? {
                        return Ok(friend_ids);
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| "Steam did not send the friend list in time.".to_string())?
}

fn decode_friend_ids(emsg: EMsg, body: &[u8]) -> Result<Option<Vec<u64>>, String> {
    if emsg != EMsg::CLIENT_FRIENDS_LIST {
        return Ok(None);
    }
    let message = CMsgClientFriendsList::decode(body).map_err(|error| error.to_string())?;
    Ok(Some(
        message
            .friends
            .into_iter()
            .filter(|friend| friend.efriendrelationship == Some(3))
            .filter_map(|friend| friend.ulfriendid)
            .collect(),
    ))
}

async fn get_preference_friend_ids(
    client: &steamroom::client::SteamClient<steamroom::client::LoggedIn>,
    steam_id: u64,
) -> Result<Vec<u64>, String> {
    let response = client
        .call_service_method(
            "Player.GetPerFriendPreferences#1",
            &CPlayerGetPerFriendPreferencesRequest {}.encode_to_vec(),
        )
        .await
        .map_err(|error| error.to_string())?;
    let response: CPlayerGetPerFriendPreferencesResponse =
        response.decode().map_err(|error| error.to_string())?;
    let steam_id_prefix = steam_id & !u64::from(u32::MAX);
    Ok(response
        .preferences
        .into_iter()
        .filter_map(|preference| preference.accountid)
        .filter(|account_id| *account_id > 0)
        .map(|account_id| steam_id_prefix | u64::from(account_id))
        .collect())
}

async fn generate_access_token(refresh_token: &str, steam_id: u64) -> Result<String, String> {
    let steam_id = steam_id.to_string();
    let response = reqwest::Client::new()
        .post(ACCESS_TOKEN_URL)
        .form(&[
            ("refresh_token", refresh_token),
            ("steamid", steam_id.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "Steam access-token refresh failed.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Steam access-token refresh returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| "Steam returned invalid access-token data.".to_string())?;
    value
        .pointer("/response/access_token")
        .or_else(|| value.get("access_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Steam did not return an access token.".to_string())
}

async fn get_oauth_friend_ids(access_token: &str) -> Result<Vec<u64>, String> {
    let response = reqwest::Client::new()
        .get(FRIEND_LIST_URL)
        .query(&[("access_token", access_token)])
        .send()
        .await
        .map_err(|_| "Steam friend-list request failed.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Steam friend-list request returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| "Steam returned invalid friend-list data.".to_string())?;
    let friends = value
        .pointer("/friends")
        .or_else(|| value.pointer("/response/friends"))
        .or_else(|| value.pointer("/friendslist/friends"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Steam did not return a friend list.".to_string())?;
    Ok(friends
        .iter()
        .filter_map(|friend| friend.get("steamid"))
        .filter_map(|steam_id| {
            steam_id
                .as_str()
                .and_then(|steam_id| steam_id.parse::<u64>().ok())
                .or_else(|| steam_id.as_u64())
        })
        .collect())
}

async fn get_player_profiles(
    client: &steamroom::client::SteamClient<steamroom::client::LoggedIn>,
    steam_ids: &[u64],
) -> Result<Vec<SteamProfile>, String> {
    let mut profiles = Vec::new();
    for steam_id_batch in steam_ids.chunks(100) {
        let request = CPlayerGetPlayerLinkDetailsRequest {
            steamids: steam_id_batch.to_vec(),
        };
        let response = client
            .call_service_method("Player.GetPlayerLinkDetails#1", &request.encode_to_vec())
            .await
            .map_err(|error| error.to_string())?;
        let response: CPlayerGetPlayerLinkDetailsResponse =
            response.decode().map_err(|error| error.to_string())?;
        profiles.extend(
            response
                .accounts
                .into_iter()
                .filter_map(|account| account.public_data)
                .map(|public| SteamProfile {
                    steam_id: public.steamid.to_string(),
                    display_name: public
                        .persona_name
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| "Steam user".to_string()),
                    avatar_url: public.sha_digest_avatar.as_deref().and_then(avatar_url),
                }),
        );
    }
    Ok(profiles)
}

fn avatar_url(hash: &[u8]) -> Option<String> {
    if hash.is_empty() || hash.iter().all(|byte| *byte == 0) {
        return None;
    }
    let hash = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("https://avatars.steamstatic.com/{hash}_medium.jpg"))
}

fn steam_id_from_token(token: &str) -> Result<u64, String> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| "Steam returned an unrecognised access token".to_string())?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Steam returned an unrecognised access token".to_string())?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|_| "Steam returned an unrecognised access token".to_string())?;
    claims
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .and_then(|subject| subject.parse().ok())
        .ok_or_else(|| "Steam access token did not identify an account".to_string())
}

fn qr_data_url(challenge_url: &str) -> Result<String, String> {
    let code = QrCode::new(challenge_url.as_bytes()).map_err(|error| error.to_string())?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(320, 320)
        .dark_color(svg::Color("#17212b"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(image)
    ))
}

fn sanitise_error(error: &str) -> String {
    let first_line = error.lines().next().unwrap_or("Unknown error");
    let end = first_line
        .char_indices()
        .nth(240)
        .map(|(index, _)| index)
        .unwrap_or(first_line.len());
    if end < first_line.len() {
        format!("{}…", &first_line[..end])
    } else {
        first_line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_renderer_does_not_expose_challenge_in_data_url() {
        let challenge = "https://s.team/q/1/12345";
        let rendered = qr_data_url(challenge).expect("QR should render");
        assert!(rendered.starts_with("data:image/svg+xml;base64,"));
        assert!(!rendered.contains(challenge));
    }

    #[test]
    fn sanitised_errors_are_single_line_and_utf8_safe() {
        let long = "🦀".repeat(300);
        let sanitised = sanitise_error(&format!("{long}\nsecret"));
        assert!(!sanitised.contains("secret"));
        assert!(sanitised.ends_with('…'));
    }

    #[test]
    fn owned_app_ids_drop_invalid_values_and_deduplicate() {
        assert_eq!(
            normalise_owned_app_ids([Some(123), Some(0), None, Some(-4), Some(123), Some(456)]),
            [123, 456].into_iter().collect()
        );
    }
}
