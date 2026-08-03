use crate::credential_store;
use crate::state::{AppState, EntitlementSummary, EntitlementSyncView, EntitlementView};
use futures_util::stream::{self, StreamExt};
use reqwest::header::{ACCEPT, COOKIE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

const ORDER_LIST_URL: &str = "https://www.humblebundle.com/api/v1/user/order";
const ORDERS_URL: &str = "https://www.humblebundle.com/api/v1/orders";
const ORDER_BATCH_SIZE: usize = 25;
const CACHE_VERSION: u32 = 2;
const CACHE_TTL_SECONDS: u64 = 24 * 60 * 60;
const CHOICE_PAGE_CONCURRENCY: usize = 6;

#[derive(Deserialize, Serialize)]
struct EntitlementCache {
    version: u32,
    refreshed_at: u64,
    total_orders: usize,
    items: Vec<EntitlementView>,
}

struct ChoiceOrder {
    order_key: String,
    parent_name: String,
    choice_url: String,
    fallback_items: Vec<EntitlementView>,
}

#[derive(Default)]
struct ParsedOrderDetails {
    entitlements: Vec<EntitlementView>,
    choice_orders: Vec<ChoiceOrder>,
}

#[derive(Debug)]
enum HumbleApiError {
    Authentication(String),
    Request(String),
    Response(String),
}

#[derive(Debug)]
pub enum HumbleSessionValidationError {
    Expired,
    Unavailable(String),
}

impl std::fmt::Display for HumbleSessionValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired => formatter.write_str("The Humble session expired. Sign in again."),
            Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl std::fmt::Display for HumbleApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authentication(message) | Self::Request(message) | Self::Response(message) => {
                formatter.write_str(message)
            }
        }
    }
}

pub async fn validate_humble_session(session: &str) -> Result<(), HumbleSessionValidationError> {
    let client = build_client().map_err(|error| {
        HumbleSessionValidationError::Unavailable(sanitise_error(&error.to_string()))
    })?;
    match request_json(&client, ORDER_LIST_URL, session, &[]).await {
        Ok(Value::Array(_)) => Ok(()),
        Ok(_) => Err(HumbleSessionValidationError::Unavailable(
            "Humble changed the account response format.".to_string(),
        )),
        Err(HumbleApiError::Authentication(_)) => Err(HumbleSessionValidationError::Expired),
        Err(error) => Err(HumbleSessionValidationError::Unavailable(sanitise_error(
            &error.to_string(),
        ))),
    }
}

#[tauri::command]
pub async fn refresh_humble_entitlements(state: State<'_, AppState>) -> Result<(), String> {
    start_refresh(state.inner().clone()).await
}

pub async fn load_cached_or_refresh(state: AppState) -> Result<(), String> {
    if let Ok(Some(mut cache)) = load_cache().await {
        let now = unix_time();
        if cache.version == CACHE_VERSION && cache_is_fresh(cache.refreshed_at, now) {
            prefer_choice_membership_urls(&mut cache.items);
            let summary = summarise(&cache.items);
            #[cfg(debug_assertions)]
            eprintln!(
                "HUMBLE_ENTITLEMENT_CACHE={{\"items\":{},\"ageSeconds\":{}}}",
                cache.items.len(),
                now.saturating_sub(cache.refreshed_at)
            );
            state
                .update_view(|view| {
                    view.entitlements = EntitlementSyncView {
                        phase: "complete".to_string(),
                        message: format!(
                            "{} available Steam entitlements loaded from cache.",
                            summary.available
                        ),
                        error: None,
                        completed_orders: cache.total_orders,
                        total_orders: cache.total_orders,
                        refreshed_at: Some(cache.refreshed_at),
                        summary,
                        items: cache.items,
                    };
                })
                .await;
            crate::wishlists::start_refresh(state).await?;
            return Ok(());
        }
    }
    start_refresh(state).await
}

pub async fn start_refresh(state: AppState) -> Result<(), String> {
    if let Some(task) = state.steam_review_task.lock().await.take() {
        task.abort();
    }
    let mut task_slot = state.humble_task.lock().await;
    if let Some(task) = task_slot.take() {
        task.abort();
    }
    state
        .update_view(|view| {
            view.steam_reviews = Default::default();
            view.entitlements = EntitlementSyncView {
                phase: "loading".to_string(),
                message: "Loading your Humble order list…".to_string(),
                ..Default::default()
            };
        })
        .await;

    let task_state = state.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(error) = run_refresh(&task_state).await {
            #[cfg(debug_assertions)]
            eprintln!("HUMBLE_SYNC_ERROR={}", sanitise_error(&error.to_string()));
            if matches!(error, HumbleApiError::Authentication(_)) {
                *task_state.humble_session.write().await = None;
                let _ = credential_store::delete_humble().await;
                task_state
                    .update_view(|view| {
                        view.humble.phase = "error".to_string();
                        view.humble.message = "Reconnect Humble to continue.".to_string();
                        view.humble.error = Some(error.to_string());
                        view.humble.remembered = false;
                    })
                    .await;
            }
            task_state
                .update_view(|view| {
                    view.entitlements.phase = "error".to_string();
                    view.entitlements.message = "Humble entitlement loading stopped.".to_string();
                    view.entitlements.error = Some(error.to_string());
                })
                .await;
        }
    });
    *task_slot = Some(task);
    Ok(())
}

#[tauri::command]
pub async fn cancel_humble_refresh(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(task) = state.humble_task.lock().await.take() {
        task.abort();
    }
    state
        .update_view(|view| {
            view.entitlements.phase = "idle".to_string();
            view.entitlements.message = "Entitlement refresh cancelled.".to_string();
        })
        .await;
    Ok(())
}

async fn run_refresh(state: &AppState) -> Result<(), HumbleApiError> {
    let session = state.humble_session.read().await.clone().ok_or_else(|| {
        HumbleApiError::Authentication("The Humble session is not connected.".to_string())
    })?;
    let client = build_client()?;

    let order_list = request_json(&client, ORDER_LIST_URL, &session, &[]).await?;
    let order_keys = parse_order_keys(&order_list)?;
    let total_orders = order_keys.len();
    #[cfg(debug_assertions)]
    eprintln!("HUMBLE_ORDER_COUNT={total_orders}");
    state
        .update_view(|view| {
            view.entitlements.total_orders = total_orders;
            view.entitlements.message = if total_orders == 1 {
                "Loading 1 Humble purchase…".to_string()
            } else {
                format!("Loading {total_orders} Humble purchases…")
            };
        })
        .await;

    let mut entitlements = Vec::new();
    let mut choice_orders = Vec::new();
    let mut completed_orders = 0;
    for order_batch in order_keys.chunks(ORDER_BATCH_SIZE) {
        let mut query = vec![("all_tpkds", "true")];
        query.extend(
            order_batch
                .iter()
                .map(|order_key| ("gamekeys", order_key.as_str())),
        );
        let details = request_json(&client, ORDERS_URL, &session, &query).await?;
        let parsed = parse_order_details(&details);
        entitlements.extend(parsed.entitlements);
        choice_orders.extend(parsed.choice_orders);
        completed_orders += order_batch.len();
        state
            .update_view(|view| {
                view.entitlements.completed_orders = completed_orders;
                view.entitlements.message =
                    format!("Loaded {completed_orders} of {total_orders} purchases…");
            })
            .await;
    }

    if !choice_orders.is_empty() {
        let choice_order_count = choice_orders.len();
        state
            .update_view(|view| {
                view.entitlements.message = format!(
                    "Loading complete game lists for {choice_order_count} Humble Choice months…"
                );
            })
            .await;
        let choice_entitlements = stream::iter(choice_orders.into_iter().map(|choice_order| {
            let client = client.clone();
            let session = session.clone();
            async move { load_choice_entitlements(&client, &session, choice_order).await }
        }))
        .buffer_unordered(CHOICE_PAGE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        entitlements.extend(choice_entitlements.into_iter().flatten());
    }

    entitlements.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    let summary = summarise(&entitlements);
    #[cfg(debug_assertions)]
    eprintln!(
        "HUMBLE_ENTITLEMENT_SUMMARY={{\"total\":{},\"available\":{},\"needsMapping\":{},\"revealed\":{},\"excluded\":{}}}",
        summary.total, summary.available, summary.needs_mapping, summary.revealed, summary.excluded
    );
    let refreshed_at = unix_time();
    let cache = EntitlementCache {
        version: CACHE_VERSION,
        refreshed_at,
        total_orders,
        items: entitlements.clone(),
    };
    if let Err(_error) = save_cache(cache).await {
        #[cfg(debug_assertions)]
        eprintln!("HUMBLE_CACHE_WRITE_ERROR={}", sanitise_error(&_error));
    }
    state
        .update_view(|view| {
            view.entitlements.phase = "complete".to_string();
            view.entitlements.message =
                format!("{} available Steam entitlements found.", summary.available);
            view.entitlements.error = None;
            view.entitlements.completed_orders = total_orders;
            view.entitlements.total_orders = total_orders;
            view.entitlements.refreshed_at = Some(refreshed_at);
            view.entitlements.summary = summary;
            view.entitlements.items = entitlements;
        })
        .await;
    crate::wishlists::start_refresh(state.clone())
        .await
        .map_err(HumbleApiError::Request)?;
    Ok(())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_is_fresh(refreshed_at: u64, now: u64) -> bool {
    refreshed_at > 0
        && refreshed_at <= now.saturating_add(5 * 60)
        && now.saturating_sub(refreshed_at) < CACHE_TTL_SECONDS
}

async fn load_cache() -> Result<Option<EntitlementCache>, String> {
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || match std::fs::read(&path) {
        Ok(bytes) => {
            protect_cache_file(&path)?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "The Humble entitlement cache is unreadable.".to_string())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Could not read the Humble entitlement cache: {error}"
        )),
    })
    .await
    .map_err(|error| format!("Humble cache task failed: {error}"))?
}

async fn save_cache(cache: EntitlementCache) -> Result<(), String> {
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || {
        let parent = path
            .parent()
            .ok_or_else(|| "Humble cache path has no parent.".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the Humble cache directory: {error}"))?;
        let encoded = serde_json::to_vec(&cache)
            .map_err(|_| "Could not encode the Humble entitlement cache.".to_string())?;
        let mut file = open_cache_file(&path)?;
        file.write_all(&encoded)
            .map_err(|error| format!("Could not save the Humble entitlement cache: {error}"))
    })
    .await
    .map_err(|error| format!("Humble cache task failed: {error}"))?
}

fn open_cache_file(path: &std::path::Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("Could not create the Humble entitlement cache: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not protect the Humble entitlement cache: {error}"))?;
    }
    Ok(file)
}

#[cfg(unix)]
fn protect_cache_file(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not protect the Humble entitlement cache: {error}"))
}

#[cfg(not(unix))]
fn protect_cache_file(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

pub async fn delete_cache() -> Result<(), String> {
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not remove the Humble entitlement cache: {error}"
        )),
    })
    .await
    .map_err(|error| format!("Humble cache task failed: {error}"))?
}

fn cache_path() -> Result<PathBuf, String> {
    dirs_next::data_local_dir()
        .map(|directory| {
            directory
                .join("Humble Gift Matcher")
                .join("humble-entitlements-v1.json")
        })
        .ok_or_else(|| "Could not locate the local application-data directory.".to_string())
}

fn build_client() -> Result<reqwest::Client, HumbleApiError> {
    reqwest::Client::builder()
        .user_agent("Humble Gift Matcher/1.0")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| HumbleApiError::Request(sanitise_error(&error.to_string())))
}

async fn request_json(
    client: &reqwest::Client,
    url: &str,
    session: &str,
    query: &[(&str, &str)],
) -> Result<Value, HumbleApiError> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .header("X-Requested-By", "hb_android_app")
        .header(COOKIE, format!("_simpleauth_sess={session}"))
        .query(&[("ajax", "true")])
        .query(query)
        .send()
        .await
        .map_err(|error| HumbleApiError::Request(sanitise_error(&error.to_string())))?;
    let status = response.status();
    let redirected_to_login = response.url().path().trim_end_matches('/') == "/login";
    if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
        || redirected_to_login
    {
        return Err(HumbleApiError::Authentication(
            "The Humble session expired. Sign in again.".to_string(),
        ));
    }
    if !status.is_success() {
        return Err(HumbleApiError::Request(format!(
            "Humble returned HTTP {}.",
            status.as_u16()
        )));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| HumbleApiError::Response("Humble returned invalid JSON.".to_string()))?;
    if value
        .get("error_id")
        .and_then(Value::as_str)
        .is_some_and(|error| error == "login_required")
    {
        return Err(HumbleApiError::Authentication(
            "The Humble session expired. Sign in again.".to_string(),
        ));
    }
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(HumbleApiError::Response(
            "Humble rejected the entitlement request.".to_string(),
        ));
    }
    Ok(value)
}

async fn request_choice_page(
    client: &reqwest::Client,
    session: &str,
    choice_url: &str,
) -> Result<String, HumbleApiError> {
    let url = format!("https://www.humblebundle.com/membership/{choice_url}");
    let response = client
        .get(url)
        .header(ACCEPT, "text/html")
        .header(COOKIE, format!("_simpleauth_sess={session}"))
        .send()
        .await
        .map_err(|error| HumbleApiError::Request(sanitise_error(&error.to_string())))?;
    let status = response.status();
    let redirected_to_login = response.url().path().trim_end_matches('/') == "/login";
    if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
        || redirected_to_login
    {
        return Err(HumbleApiError::Authentication(
            "The Humble session expired. Sign in again.".to_string(),
        ));
    }
    if !status.is_success() {
        return Err(HumbleApiError::Request(format!(
            "Humble returned HTTP {} for a Choice page.",
            status.as_u16()
        )));
    }
    response.text().await.map_err(|_| {
        HumbleApiError::Response("Humble returned an unreadable Choice page.".to_string())
    })
}

async fn load_choice_entitlements(
    client: &reqwest::Client,
    session: &str,
    choice_order: ChoiceOrder,
) -> Vec<EntitlementView> {
    let membership_url = format!(
        "https://www.humblebundle.com/membership/{}",
        choice_order.choice_url
    );
    let mut items = match request_choice_page(client, session, &choice_order.choice_url).await {
        Ok(page) => {
            match parse_choice_page(&page, &choice_order.order_key, &choice_order.parent_name) {
                Ok(items) => items,
                Err(_error) => {
                    #[cfg(debug_assertions)]
                    eprintln!("HUMBLE_CHOICE_PARSE_ERROR={}", sanitise_error(&_error));
                    choice_order.fallback_items
                }
            }
        }
        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!(
                "HUMBLE_CHOICE_FETCH_ERROR={}",
                sanitise_error(&_error.to_string())
            );
            choice_order.fallback_items
        }
    };
    for item in &mut items {
        item.purchase_url = Some(membership_url.clone());
    }
    items
}

fn parse_order_keys(value: &Value) -> Result<Vec<String>, HumbleApiError> {
    let orders = value.as_array().ok_or_else(|| {
        HumbleApiError::Response("Humble changed the order-list response format.".to_string())
    })?;
    let mut keys = orders
        .iter()
        .filter_map(|order| order.get("gamekey").and_then(Value::as_str))
        .filter(|key| is_safe_order_key(key))
        .map(str::to_string)
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn parse_order_details(value: &Value) -> ParsedOrderDetails {
    let Some(orders) = value.as_object() else {
        return ParsedOrderDetails::default();
    };
    let mut parsed = ParsedOrderDetails::default();
    for (order_key, order) in orders {
        let parent_name = order
            .pointer("/product/human_name")
            .and_then(Value::as_str)
            .or_else(|| {
                order
                    .pointer("/product/machine_name")
                    .and_then(Value::as_str)
            })
            .unwrap_or("Humble purchase")
            .to_string();
        let Some(items) = order
            .pointer("/tpkd_dict/all_tpks")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let fallback_items = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                parse_entitlement(order_key, index, &parent_name, item.as_object())
            })
            .collect::<Vec<_>>();
        let choice_url = order
            .pointer("/product/choice_url")
            .and_then(Value::as_str)
            .filter(|choice_url| is_safe_choice_url(choice_url));
        if let Some(choice_url) = choice_url {
            parsed.choice_orders.push(ChoiceOrder {
                order_key: order_key.clone(),
                parent_name,
                choice_url: choice_url.to_string(),
                fallback_items,
            });
            continue;
        }
        parsed.entitlements.extend(fallback_items);
    }
    parsed
}

fn parse_choice_page(
    page: &str,
    order_key: &str,
    parent_name: &str,
) -> Result<Vec<EntitlementView>, String> {
    const MARKER: &str = "id=\"webpack-monthly-product-data\"";
    let marker_index = page
        .find(MARKER)
        .ok_or_else(|| "Humble Choice page data was not found.".to_string())?;
    let content_start = page[marker_index..]
        .find('>')
        .map(|offset| marker_index + offset + 1)
        .ok_or_else(|| "Humble Choice page data was malformed.".to_string())?;
    let content_end = page[content_start..]
        .find("</script>")
        .map(|offset| content_start + offset)
        .ok_or_else(|| "Humble Choice page data was incomplete.".to_string())?;
    let value = serde_json::from_str::<Value>(&page[content_start..content_end])
        .map_err(|_| "Humble Choice page data was invalid.".to_string())?;
    let games = value
        .pointer("/contentChoiceOptions/contentChoiceData/game_data")
        .and_then(Value::as_object)
        .ok_or_else(|| "Humble Choice game data was unavailable.".to_string())?;
    let mut entitlements = Vec::new();
    for game in games.values() {
        let Some(items) = game.get("tpkds").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if let Some(item) =
                parse_entitlement(order_key, entitlements.len(), parent_name, item.as_object())
            {
                entitlements.push(item);
            }
        }
    }
    if entitlements.is_empty() {
        return Err("Humble Choice page contained no entitlements.".to_string());
    }
    Ok(entitlements)
}

fn parse_entitlement(
    order_key: &str,
    index: usize,
    parent_name: &str,
    item: Option<&Map<String, Value>>,
) -> Option<EntitlementView> {
    let item = item?;
    let name = string_field(item, "human_name")
        .or_else(|| string_field(item, "machine_name"))
        .unwrap_or("Unnamed Humble entitlement")
        .to_string();
    let key_type = string_field(item, "key_type").unwrap_or("unknown");
    let key_type_label = string_field(item, "key_type_human_name")
        .unwrap_or(key_type)
        .to_string();
    let steam_app_id = parse_app_id(item.get("steam_app_id"));
    let is_steam = key_type.to_lowercase().contains("steam")
        || key_type_label.to_lowercase().contains("steam");
    let revealed = item.contains_key("redeemed_key_val");
    let hidden = bool_field(item, "visible") == Some(false);
    let expired =
        bool_field(item, "is_expired") == Some(true) || bool_field(item, "expired") == Some(true);
    let region_restricted = item
        .get("region_restrictions")
        .is_some_and(has_meaningful_value)
        || item.get("regions").is_some_and(has_meaningful_value);
    let package_ambiguity = bool_field(item, "is_dlc") == Some(true)
        || name.to_lowercase().contains("dlc")
        || name.to_lowercase().contains("deluxe")
        || name.to_lowercase().contains("upgrade")
        || key_type.to_lowercase().contains("package");

    let mut reasons = Vec::new();
    let status = if !is_steam {
        reasons.push(format!("Key type is {key_type_label}, not Steam."));
        "not_steam"
    } else if hidden {
        reasons.push("Humble marks this entitlement as hidden.".to_string());
        "hidden"
    } else if expired {
        reasons.push("Humble marks this entitlement as expired.".to_string());
        "expired"
    } else if revealed {
        reasons.push("The key value has already been revealed on Humble.".to_string());
        "revealed"
    } else if steam_app_id.is_none() {
        reasons.push("Humble did not provide a valid Steam AppID.".to_string());
        "needs_mapping"
    } else {
        reasons.push("Visible, unrevealed Steam entitlement.".to_string());
        "available"
    };
    if region_restricted {
        reasons.push("Region restrictions may apply.".to_string());
    }
    if package_ambiguity {
        reasons.push(
            "The AppID may identify only the base game rather than this exact edition or content."
                .to_string(),
        );
    }

    Some(EntitlementView {
        id: format!("{:x}-{index}", stable_hash(order_key.as_bytes())),
        mapping_key: format!(
            "{:x}",
            stable_hash(
                string_field(item, "machine_name")
                    .unwrap_or(&name)
                    .as_bytes()
            )
        ),
        name,
        parent_name: parent_name.to_string(),
        steam_app_id,
        steam_name: steam_app_id.map(|_| {
            string_field(item, "human_name")
                .or_else(|| string_field(item, "machine_name"))
                .unwrap_or("Steam game")
                .to_string()
        }),
        mapping_source: steam_app_id.map(|_| "humble".to_string()),
        mapping_candidates: Vec::new(),
        key_type_label,
        status: status.to_string(),
        reasons,
        purchase_url: is_safe_order_key(order_key)
            .then(|| format!("https://www.humblebundle.com/downloads?key={order_key}")),
        region_restricted,
        package_ambiguity,
    })
}

fn string_field<'a>(item: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    item.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn bool_field(item: &Map<String, Value>, name: &str) -> Option<bool> {
    item.get(name).and_then(Value::as_bool)
}

fn parse_app_id(value: Option<&Value>) -> Option<u32> {
    let app_id = match value? {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => value.parse::<u32>().ok(),
        _ => None,
    }?;
    (app_id > 0).then_some(app_id)
}

fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Number(_) => true,
    }
}

fn prefer_choice_membership_urls(entitlements: &mut [EntitlementView]) {
    for item in entitlements {
        if let Some(url) = choice_membership_url(&item.parent_name) {
            item.purchase_url = Some(url);
        }
    }
}

fn choice_membership_url(parent_name: &str) -> Option<String> {
    let lower = parent_name.to_ascii_lowercase();
    // Humble's surviving month archive is a Choice feature. Pre-Choice
    // Humble Monthly pages now resolve to a 404, so retain their specific
    // authenticated `/downloads?key=…` purchase URL instead.
    if !lower.contains("humble choice") {
        return None;
    }
    let words = lower.split_whitespace().collect::<Vec<_>>();
    let month = words
        .first()?
        .trim_matches(|character: char| !character.is_ascii_alphabetic());
    let year = words
        .get(1)?
        .trim_matches(|character: char| !character.is_ascii_digit());
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    (MONTHS.contains(&month) && year.len() == 4 && year.chars().all(|char| char.is_ascii_digit()))
        .then(|| format!("https://www.humblebundle.com/membership/{month}-{year}"))
}

pub(crate) fn summarise(entitlements: &[EntitlementView]) -> EntitlementSummary {
    EntitlementSummary {
        total: entitlements.len(),
        available: entitlements
            .iter()
            .filter(|item| item.status == "available" || item.status == "needs_mapping")
            .count(),
        needs_mapping: entitlements
            .iter()
            .filter(|item| item.status == "needs_mapping")
            .count(),
        revealed: entitlements
            .iter()
            .filter(|item| item.status == "revealed")
            .count(),
        excluded: entitlements
            .iter()
            .filter(|item| matches!(item.status.as_str(), "expired" | "hidden" | "not_steam"))
            .count(),
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "available" => 0,
        "needs_mapping" => 1,
        "revealed" => 2,
        "expired" => 3,
        "hidden" => 4,
        _ => 5,
    }
}

fn is_safe_order_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn is_safe_choice_url(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn stable_hash(value: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn sanitise_error(error: &str) -> String {
    error
        .lines()
        .next()
        .unwrap_or("Unknown Humble request error")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::json!({
            "safe-order-key": {
                "product": { "human_name": "Test Bundle" },
                "tpkd_dict": {
                    "all_tpks": [
                        {
                            "human_name": "Available Game",
                            "key_type": "steam",
                            "key_type_human_name": "Steam",
                            "steam_app_id": 123,
                            "visible": true
                        },
                        {
                            "human_name": "Revealed Game",
                            "key_type": "steam",
                            "steam_app_id": "456",
                            "redeemed_key_val": "AAAAA-BBBBB-CCCCC"
                        },
                        {
                            "human_name": "Ambiguous Deluxe Edition",
                            "key_type": "steam",
                            "region_restrictions": ["GB"]
                        },
                        {
                            "human_name": "Soundtrack",
                            "key_type": "download"
                        }
                    ]
                }
            }
        })
    }

    #[test]
    fn parser_classifies_entitlements_without_retaining_key_values() {
        let parsed = parse_order_details(&fixture());
        let summary = summarise(&parsed.entitlements);
        assert_eq!(parsed.entitlements.len(), 4);
        assert!(parsed.choice_orders.is_empty());
        assert_eq!(summary.available, 2);
        assert_eq!(summary.needs_mapping, 1);
        assert_eq!(summary.revealed, 1);
        assert_eq!(summary.excluded, 1);
        assert_eq!(parsed.entitlements[0].steam_app_id, Some(123));
        let encoded = serde_json::to_string(&parsed.entitlements).unwrap();
        assert!(!encoded.contains("AAAAA-BBBBB-CCCCC"));
        assert!(!encoded.contains("redeemed_key_val"));
    }

    #[test]
    fn choice_page_parser_includes_unrevealed_games_without_retaining_secrets() {
        let monthly_data = serde_json::json!({
            "contentChoiceOptions": {
                "contentChoiceData": {
                    "game_data": {
                        "neonwhite": {
                            "title": "Neon White",
                            "tpkds": [{
                                "human_name": "Neon White",
                                "machine_name": "neonwhite_row_choice_steam",
                                "key_type": "steam",
                                "key_type_human_name": "Steam",
                                "steam_app_id": 1533420,
                                "visible": true,
                                "redeemed_key_val": "SECRET-KEY",
                                "gamekey": "safe-order-key"
                            }]
                        },
                        "tunic": {
                            "title": "TUNIC",
                            "tpkds": [{
                                "human_name": "Tunic",
                                "machine_name": "tunic_choice_steam",
                                "key_type": "steam",
                                "steam_app_id": 553420,
                                "visible": true,
                                "gamekey": "safe-order-key"
                            }]
                        }
                    }
                }
            },
            "csrfToken": "SECRET-CSRF"
        });
        let page = format!(
            "<html><script id=\"webpack-monthly-product-data\" type=\"application/json\">{monthly_data}</script></html>"
        );
        let parsed = parse_choice_page(&page, "safe-order-key", "July 2026 Humble Choice").unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed
                .iter()
                .find(|item| item.name == "Tunic")
                .unwrap()
                .status,
            "available"
        );
        assert_eq!(
            parsed
                .iter()
                .find(|item| item.name == "Neon White")
                .unwrap()
                .status,
            "revealed"
        );
        let encoded = serde_json::to_string(&parsed).unwrap();
        assert!(!encoded.contains("SECRET-KEY"));
        assert!(!encoded.contains("SECRET-CSRF"));
    }

    #[test]
    fn missing_or_zero_app_ids_are_not_accepted() {
        assert_eq!(parse_app_id(Some(&Value::from(0))), None);
        assert_eq!(parse_app_id(Some(&Value::from("not-an-id"))), None);
        assert_eq!(parse_app_id(Some(&Value::from("42"))), Some(42));
    }

    #[test]
    fn order_keys_are_deduplicated_and_constrained() {
        let orders = serde_json::json!([
            { "gamekey": "safe-key" },
            { "gamekey": "safe-key" },
            { "gamekey": "../unsafe" },
            { "other": true }
        ]);
        assert_eq!(parse_order_keys(&orders).unwrap(), vec!["safe-key"]);
    }

    #[test]
    fn choice_page_paths_are_constrained() {
        assert!(is_safe_choice_url("july-2026"));
        assert!(!is_safe_choice_url("../account"));
        assert!(!is_safe_choice_url("https://example.com"));
    }

    #[test]
    fn choice_entitlements_link_to_their_month_page() {
        assert_eq!(
            choice_membership_url("July 2026 Humble Choice").as_deref(),
            Some("https://www.humblebundle.com/membership/july-2026")
        );
        assert_eq!(
            choice_membership_url("November 2019 Humble Monthly").as_deref(),
            None
        );
        assert!(choice_membership_url("Humble Indie Bundle").is_none());
    }

    #[test]
    fn entitlement_cache_expires_after_twenty_four_hours() {
        let refreshed_at = 1_000_000;
        assert!(cache_is_fresh(
            refreshed_at,
            refreshed_at + CACHE_TTL_SECONDS - 1
        ));
        assert!(!cache_is_fresh(
            refreshed_at,
            refreshed_at + CACHE_TTL_SECONDS
        ));
        assert!(!cache_is_fresh(0, refreshed_at));
    }
}
