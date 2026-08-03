use crate::state::{
    AppState, GiftMatchView, MappingCandidateView, WishlistGameView, WishlistPersonView,
    WishlistSyncView,
};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use steamroom::apps::AccessToken;
use steamroom::depot::AppId;
use steamroom::types::{KeyValue, KvValue};
use tauri::State;

const WISHLIST_URL: &str = "https://api.steampowered.com/IWishlistService/GetWishlist/v1/";
const STORE_SEARCH_URL: &str = "https://store.steampowered.com/api/storesearch/";
const CACHE_VERSION: u32 = 1;
const MAPPING_CACHE_VERSION: u32 = 2;
const STORE_SEARCH_CACHE_VERSION: u32 = 2;
const STORE_SEARCH_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppIdentity {
    app_id: u32,
    name: String,
    app_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedMapping {
    app_id: u32,
    steam_name: String,
    source: String,
}

#[derive(Default, Deserialize, Serialize)]
struct MappingFile {
    version: u32,
    records: BTreeMap<String, SavedMapping>,
}

#[derive(Default, Deserialize, Serialize)]
struct IdentityFile {
    version: u32,
    records: BTreeMap<u32, AppIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoreSearchRecord {
    query: String,
    checked_at: u64,
    candidates: Vec<MappingCandidateView>,
}

#[derive(Default, Deserialize, Serialize)]
struct StoreSearchFile {
    version: u32,
    records: BTreeMap<String, StoreSearchRecord>,
}

struct WishlistResult {
    person: WishlistPersonView,
    app_ids: BTreeSet<u32>,
    temporary_error: Option<String>,
}

#[tauri::command]
pub async fn refresh_wishlists(state: State<'_, AppState>) -> Result<(), String> {
    start_refresh(state.inner().clone()).await
}

#[tauri::command]
pub async fn search_steam_apps(query: String) -> Result<Vec<MappingCandidateView>, String> {
    let query = query.trim();
    if !(2..=120).contains(&query.chars().count()) {
        return Err("Enter at least two characters to search Steam.".to_string());
    }
    search_store(query).await
}

async fn search_store(query: &str) -> Result<Vec<MappingCandidateView>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Humble Gift Matcher/0.1")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "Could not reach Steam search.".to_string())?;
    let mut candidates = BTreeMap::new();
    for search_term in store_search_terms(query) {
        let response = client
            .get(STORE_SEARCH_URL)
            .query(&[
                ("term", search_term),
                ("l", "english".to_string()),
                ("cc", "GB".to_string()),
            ])
            .send()
            .await
            .map_err(|_| "Could not reach Steam search.".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Steam search returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        let value = response
            .json::<Value>()
            .await
            .map_err(|_| "Steam returned invalid search data.".to_string())?;
        for candidate in parse_store_search(&value, query) {
            candidates.entry(candidate.app_id).or_insert(candidate);
        }
        if unique_exact_candidate(&candidates.values().cloned().collect::<Vec<_>>()).is_some() {
            break;
        }
    }
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
    candidates.truncate(12);
    Ok(candidates)
}

fn store_search_terms(query: &str) -> Vec<String> {
    let mut terms = vec![query.to_string()];
    let canonical = normalise_title_tokens(query).join(" ");
    let comparison = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if !canonical.is_empty() && canonical != comparison {
        terms.push(canonical);
    }
    terms
}

fn parse_store_search(value: &Value, query: &str) -> Vec<MappingCandidateView> {
    let mut candidates = value
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("app") {
                return None;
            }
            let app_id = item
                .get("id")?
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())?;
            let name = item.get("name")?.as_str()?.trim();
            (app_id > 0 && !name.is_empty()).then(|| MappingCandidateView {
                app_id,
                name: name.to_string(),
                similarity: title_similarity(query, name),
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(Ordering::Equal)
    });
    candidates.truncate(12);
    candidates
}

#[tauri::command]
pub async fn set_entitlement_mapping(
    state: State<'_, AppState>,
    mapping_key: String,
    app_id: u32,
    steam_name: String,
) -> Result<(), String> {
    if mapping_key.is_empty() || mapping_key.len() > 128 || app_id == 0 {
        return Err("Enter a valid Steam AppID.".to_string());
    }
    let steam_name = if steam_name.trim().is_empty() {
        format!("Steam App {app_id}")
    } else {
        steam_name.trim().chars().take(240).collect()
    };
    let mapping = SavedMapping {
        app_id,
        steam_name,
        source: "manual".to_string(),
    };
    let mut mappings = load_mapping_file().await?;
    mappings
        .records
        .insert(mapping_key.clone(), mapping.clone());
    save_mapping_file(&mappings).await?;
    state
        .update_view(|view| {
            for item in view.entitlements.items.iter_mut().filter(|item| {
                item.mapping_key == mapping_key
                    && (item.status == "needs_mapping" || has_local_mapping(item))
            }) {
                apply_mapping(item, &mapping);
            }
            view.entitlements.summary = crate::entitlements::summarise(&view.entitlements.items);
            view.steam_reviews = Default::default();
        })
        .await;
    rebuild_matches(state.inner()).await;
    Ok(())
}

#[tauri::command]
pub async fn clear_entitlement_mapping(
    state: State<'_, AppState>,
    mapping_key: String,
) -> Result<(), String> {
    let mut mappings = load_mapping_file().await?;
    mappings.records.remove(&mapping_key);
    save_mapping_file(&mappings).await?;
    state
        .update_view(|view| {
            for item in view
                .entitlements
                .items
                .iter_mut()
                .filter(|item| item.mapping_key == mapping_key && has_local_mapping(item))
            {
                item.steam_app_id = None;
                item.steam_name = None;
                item.mapping_source = None;
                item.status = "needs_mapping".to_string();
                item.reasons.retain(|reason| {
                    !reason.contains("title match") && !reason.contains("corrected locally")
                });
                if !item
                    .reasons
                    .iter()
                    .any(|reason| reason.contains("did not provide a valid Steam AppID"))
                {
                    item.reasons
                        .push("Humble did not provide a valid Steam AppID.".to_string());
                }
            }
            view.entitlements.summary = crate::entitlements::summarise(&view.entitlements.items);
            view.steam_reviews = Default::default();
        })
        .await;
    rebuild_matches(state.inner()).await;
    Ok(())
}

pub async fn start_refresh(state: AppState) -> Result<(), String> {
    let prerequisites_ready = {
        let view = state.view.read().await;
        view.steam.phase == "connected"
            && view.humble.phase == "connected"
            && view.entitlements.phase == "complete"
    };
    if !prerequisites_ready {
        return Ok(());
    }

    let mut task_slot = state.wishlist_task.lock().await;
    if let Some(task) = task_slot.take() {
        task.abort();
    }
    state
        .update_view(|view| {
            view.wishlists = WishlistSyncView {
                phase: "loading".to_string(),
                message: "Loading accessible Steam wishlists…".to_string(),
                ..Default::default()
            };
        })
        .await;
    let task_state = state.clone();
    *task_slot = Some(tauri::async_runtime::spawn(async move {
        if let Err(error) = run_refresh(&task_state).await {
            task_state
                .update_view(|view| {
                    view.wishlists.phase = "error".to_string();
                    view.wishlists.message = "Steam wishlist matching stopped.".to_string();
                    view.wishlists.error = Some(sanitise_error(&error));
                })
                .await;
        }
    }));
    Ok(())
}

async fn run_refresh(state: &AppState) -> Result<(), String> {
    let (client, self_profile, friend_profiles) = {
        let session = state.steam_session.read().await;
        let session = session
            .as_ref()
            .ok_or_else(|| "Steam is not connected.".to_string())?;
        let profile = state
            .view
            .read()
            .await
            .steam
            .profile
            .clone()
            .ok_or_else(|| "Steam profile is unavailable.".to_string())?;
        (session.client.clone(), profile, session.friends.clone())
    };

    let mut people = vec![WishlistPersonView {
        steam_id: self_profile.steam_id,
        display_name: self_profile.display_name,
        avatar_url: self_profile.avatar_url,
        is_self: true,
        wishlist_access: "loading".to_string(),
        wishlist_count: 0,
    }];
    people.extend(
        friend_profiles
            .into_iter()
            .map(|profile| WishlistPersonView {
                steam_id: profile.steam_id,
                display_name: profile.display_name,
                avatar_url: profile.avatar_url,
                is_self: false,
                wishlist_access: "loading".to_string(),
                wishlist_count: 0,
            }),
    );
    let people_total = people.len();
    state
        .update_view(|view| {
            view.wishlists.people_total = people_total;
            view.wishlists.message = format!(
                "Checking {people_total} Steam {}…",
                if people_total == 1 {
                    "wishlist"
                } else {
                    "wishlists"
                }
            );
        })
        .await;

    let http = reqwest::Client::builder()
        .user_agent("Humble Gift Matcher/0.1")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let results = stream::iter(people.into_iter().map(|person| {
        let http = http.clone();
        async move { load_person_wishlist(&http, person).await }
    }))
    .buffer_unordered(8)
    .collect::<Vec<_>>()
    .await;

    let mut memberships: BTreeMap<u32, Vec<WishlistPersonView>> = BTreeMap::new();
    let mut resolved_people = Vec::new();
    let mut temporary_errors = Vec::new();
    for result in results {
        if let Some(error) = result.temporary_error {
            temporary_errors.push(error);
        }
        for app_id in &result.app_ids {
            memberships
                .entry(*app_id)
                .or_default()
                .push(result.person.clone());
        }
        resolved_people.push(result.person);
    }
    resolved_people.sort_by(|left, right| {
        right.is_self.cmp(&left.is_self).then_with(|| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
        })
    });
    let accessible = resolved_people
        .iter()
        .filter(|person| person.wishlist_access == "accessible")
        .count();
    let inaccessible = resolved_people
        .iter()
        .filter(|person| person.wishlist_access == "inaccessible")
        .count();
    let wishlist_apps = memberships.len();
    *state.wishlist_memberships.write().await = memberships.clone();
    state
        .update_view(|view| {
            view.wishlists.people = resolved_people.clone();
            view.wishlists.people_accessible = accessible;
            view.wishlists.people_inaccessible = inaccessible;
            view.wishlists.wishlist_apps = wishlist_apps;
            view.wishlists.message =
                format!("Loaded {accessible} accessible Steam wishlists. Matching titles…");
        })
        .await;
    #[cfg(debug_assertions)]
    eprintln!(
        "WISHLIST_SUMMARY={{\"people\":{people_total},\"accessible\":{accessible},\"inaccessible\":{inaccessible},\"apps\":{wishlist_apps}}}"
    );

    let entitlement_app_ids = {
        let view = state.view.read().await;
        view.entitlements
            .items
            .iter()
            .filter(|item| {
                item.status == "available" && item.mapping_source.as_deref() == Some("humble")
            })
            .filter_map(|item| item.steam_app_id)
            .collect::<BTreeSet<_>>()
    };
    let identity_app_ids = memberships
        .keys()
        .copied()
        .chain(entitlement_app_ids)
        .collect::<BTreeSet<_>>();
    let identities = if identity_app_ids.is_empty() {
        Vec::new()
    } else {
        load_app_identities(state, &client, identity_app_ids.into_iter()).await?
    };
    validate_humble_mappings(state, &identities).await;
    apply_saved_mappings(state).await?;
    let unresolved_titles = {
        let view = state.view.read().await;
        view.entitlements
            .items
            .iter()
            .filter(|item| item.status == "needs_mapping")
            .count()
    };
    let wishlist_items = build_person_wishlists(&resolved_people, &memberships, &identities);
    state
        .update_view(|view| {
            view.wishlists.wishlist_items = wishlist_items;
        })
        .await;
    if unresolved_titles > 0 {
        apply_automatic_mappings(state, &identities).await?;
    }
    apply_store_exact_mappings(state).await?;
    rebuild_matches(state).await;

    state
        .update_view(|view| {
            view.wishlists.phase = "complete".to_string();
            view.wishlists.message = format!(
                "{} gift matches across {} accessible wishlists.",
                view.wishlists.matches.len(),
                view.wishlists.people_accessible
            );
            view.wishlists.error = temporary_errors
                .first()
                .map(|error| format!("Some wishlists could not be checked: {error}"));
        })
        .await;
    #[cfg(debug_assertions)]
    {
        let view = state.view.read().await;
        let automatic_mappings = view
            .entitlements
            .items
            .iter()
            .filter(|item| item.mapping_source.as_deref() == Some("automatic"))
            .count();
        eprintln!(
            "MATCH_SUMMARY={{\"matches\":{},\"automaticMappings\":{automatic_mappings}}}",
            view.wishlists.matches.len()
        );
    }
    Ok(())
}

fn build_person_wishlists(
    people: &[WishlistPersonView],
    memberships: &BTreeMap<u32, Vec<WishlistPersonView>>,
    identities: &[AppIdentity],
) -> BTreeMap<String, Vec<WishlistGameView>> {
    let names = identities
        .iter()
        .map(|identity| (identity.app_id, identity.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut wishlists = people
        .iter()
        .filter(|person| person.wishlist_access == "accessible")
        .map(|person| (person.steam_id.clone(), Vec::new()))
        .collect::<BTreeMap<_, Vec<WishlistGameView>>>();
    for (app_id, wishers) in memberships {
        let game = WishlistGameView {
            app_id: *app_id,
            name: names
                .get(app_id)
                .map(|name| (*name).to_string())
                .unwrap_or_else(|| format!("Steam App {app_id}")),
        };
        for person in wishers {
            wishlists
                .entry(person.steam_id.clone())
                .or_default()
                .push(game.clone());
        }
    }
    for games in wishlists.values_mut() {
        games.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.app_id.cmp(&right.app_id))
        });
    }
    wishlists
}

async fn load_person_wishlist(
    client: &reqwest::Client,
    mut person: WishlistPersonView,
) -> WishlistResult {
    let response = client
        .get(WISHLIST_URL)
        .query(&[("steamid", person.steam_id.as_str())])
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            person.wishlist_access = "error".to_string();
            return WishlistResult {
                person,
                app_ids: BTreeSet::new(),
                temporary_error: Some(sanitise_error(&error.to_string())),
            };
        }
    };
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        person.wishlist_access = "inaccessible".to_string();
        return WishlistResult {
            person,
            app_ids: BTreeSet::new(),
            temporary_error: None,
        };
    }
    if !response.status().is_success() {
        person.wishlist_access = "error".to_string();
        return WishlistResult {
            person,
            app_ids: BTreeSet::new(),
            temporary_error: Some(format!(
                "Steam returned HTTP {}.",
                response.status().as_u16()
            )),
        };
    }
    let value = match response.json::<Value>().await {
        Ok(value) => value,
        Err(_) => {
            person.wishlist_access = "error".to_string();
            return WishlistResult {
                person,
                app_ids: BTreeSet::new(),
                temporary_error: Some("Steam returned invalid wishlist data.".to_string()),
            };
        }
    };
    let Some(items) = value
        .pointer("/response/items")
        .and_then(Value::as_array)
        .or_else(|| value.get("items").and_then(Value::as_array))
    else {
        person.wishlist_access = "inaccessible".to_string();
        return WishlistResult {
            person,
            app_ids: BTreeSet::new(),
            temporary_error: None,
        };
    };
    let app_ids = items
        .iter()
        .filter_map(|item| item.get("appid").or_else(|| item.get("app_id")))
        .filter_map(Value::as_u64)
        .filter_map(|app_id| u32::try_from(app_id).ok())
        .filter(|app_id| *app_id > 0)
        .collect::<BTreeSet<_>>();
    person.wishlist_access = "accessible".to_string();
    person.wishlist_count = app_ids.len();
    WishlistResult {
        person,
        app_ids,
        temporary_error: None,
    }
}

async fn load_app_identities(
    state: &AppState,
    client: &steamroom::client::SteamClient<steamroom::client::LoggedIn>,
    app_ids: impl Iterator<Item = u32>,
) -> Result<Vec<AppIdentity>, String> {
    let mut cache = load_identity_file().await?;
    let requested = app_ids.collect::<BTreeSet<_>>();
    let missing = requested
        .iter()
        .filter(|app_id| !cache.records.contains_key(app_id))
        .copied()
        .collect::<Vec<_>>();
    let _protocol_guard = state.steam_protocol_gate.lock().await;
    for batch in missing.chunks(25) {
        let app_ids = batch.iter().copied().map(AppId).collect::<Vec<_>>();
        let mut tokens = client
            .pics_get_access_tokens(&app_ids)
            .await
            .map_err(|error| error.to_string())?;
        let token_ids = tokens
            .iter()
            .map(|token| token.app_id.0)
            .collect::<HashSet<_>>();
        tokens.extend(
            batch
                .iter()
                .filter(|app_id| !token_ids.contains(app_id))
                .map(|app_id| AccessToken {
                    app_id: AppId(*app_id),
                    token: 0,
                }),
        );
        let infos = client
            .pics_get_product_info(&tokens)
            .await
            .map_err(|error| error.to_string())?;
        let mut product_buffers = infos
            .into_iter()
            .filter_map(|info| Some((info.app_id?.0, info.kv_data?)))
            .collect::<BTreeMap<_, _>>();

        // Steam can split PICS replies across messages, while steamroom 0.3
        // returns the first response. Retry omitted apps individually.
        let omitted = batch
            .iter()
            .filter(|app_id| !product_buffers.contains_key(app_id))
            .copied()
            .collect::<Vec<_>>();
        for app_id in omitted {
            let token = tokens
                .iter()
                .find(|token| token.app_id.0 == app_id)
                .cloned()
                .unwrap_or(AccessToken {
                    app_id: AppId(app_id),
                    token: 0,
                });
            if let Ok(infos) = client.pics_get_product_info(&[token]).await {
                product_buffers.extend(
                    infos
                        .into_iter()
                        .filter_map(|info| Some((info.app_id?.0, info.kv_data?))),
                );
            }
        }
        for identity in product_buffers
            .into_iter()
            .filter_map(|(app_id, buffer)| parse_app_identity(app_id, &buffer).ok())
        {
            cache.records.insert(identity.app_id, identity);
        }
        state
            .update_view(|view| {
                let completed = requested.len().saturating_sub(
                    requested
                        .iter()
                        .filter(|app_id| !cache.records.contains_key(app_id))
                        .count(),
                );
                view.wishlists.message = format!(
                    "Reading Steam titles for matching ({completed}/{})…",
                    requested.len()
                );
            })
            .await;
    }
    save_identity_file(&cache).await?;
    Ok(requested
        .iter()
        .filter_map(|app_id| cache.records.get(app_id).cloned())
        .collect())
}

fn parse_app_identity(app_id: u32, buffer: &[u8]) -> Result<AppIdentity, String> {
    let root = match KeyValue::from_binary(buffer) {
        Ok(root) => root,
        Err(binary_error) => {
            let text = std::str::from_utf8(buffer).map_err(|_| {
                format!("Could not parse binary Steam app metadata: {binary_error}")
            })?;
            KeyValue::from_text(text).map_err(|error| error.to_string())?
        }
    };
    let app_info = if root.key.eq_ignore_ascii_case("appinfo") {
        &root
    } else {
        root.get("appinfo").unwrap_or(&root)
    };
    let common = app_info
        .get("common")
        .ok_or_else(|| "Steam app metadata has no common section.".to_string())?;
    let name = common
        .get("name")
        .and_then(kv_string)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "Steam app metadata has no name.".to_string())?;
    let app_type = common.get("type").and_then(kv_string).unwrap_or("unknown");
    Ok(AppIdentity {
        app_id,
        name: name.to_string(),
        app_type: app_type.to_ascii_lowercase(),
    })
}

fn kv_string(value: &KeyValue) -> Option<&str> {
    match &value.value {
        KvValue::String(value) => Some(value),
        _ => None,
    }
}

async fn validate_humble_mappings(state: &AppState, identities: &[AppIdentity]) {
    let identities = identities
        .iter()
        .map(|identity| (identity.app_id, identity))
        .collect::<BTreeMap<_, _>>();
    state
        .update_view(|view| {
            let mut changed = false;
            for item in &mut view.entitlements.items {
                if item.status != "available" || item.mapping_source.as_deref() != Some("humble") {
                    continue;
                }
                let Some(app_id) = item.steam_app_id else {
                    continue;
                };
                let Some(identity) = identities.get(&app_id) else {
                    continue;
                };
                if !humble_mapping_mismatch(&item.name, identity) {
                    continue;
                }

                let similarity = title_similarity(&item.name, &identity.name);
                item.steam_app_id = None;
                item.steam_name = None;
                item.mapping_source = None;
                item.status = "needs_mapping".to_string();
                item.mapping_candidates = vec![MappingCandidateView {
                    app_id,
                    name: identity.name.clone(),
                    similarity,
                }];
                item.reasons
                    .retain(|reason| reason != "Visible, unrevealed Steam entitlement.");
                item.reasons.push(format!(
                    "Humble supplied AppID {app_id}, but Steam identifies it as {}.",
                    identity.name
                ));
                changed = true;
            }
            if changed {
                view.entitlements.summary =
                    crate::entitlements::summarise(&view.entitlements.items);
                view.steam_reviews = Default::default();
            }
        })
        .await;
}

fn humble_mapping_mismatch(humble_name: &str, identity: &AppIdentity) -> bool {
    let similarity = title_similarity(humble_name, &identity.name);
    similarity < 0.5 || (identity.app_type != "game" && similarity < 0.999)
}

async fn apply_saved_mappings(state: &AppState) -> Result<(), String> {
    let mappings = load_mapping_file().await?;
    state
        .update_view(|view| {
            for item in &mut view.entitlements.items {
                if item.status != "needs_mapping" {
                    continue;
                }
                if let Some(mapping) = mappings.records.get(&item.mapping_key) {
                    apply_mapping(item, mapping);
                }
            }
            view.entitlements.summary = crate::entitlements::summarise(&view.entitlements.items);
        })
        .await;
    Ok(())
}

async fn apply_automatic_mappings(
    state: &AppState,
    identities: &[AppIdentity],
) -> Result<(), String> {
    let mut mappings = load_mapping_file().await?;
    state
        .update_view(|view| {
            for item in &mut view.entitlements.items {
                if item.status != "needs_mapping" {
                    continue;
                }
                let candidates = rank_candidates(&item.name, identities);
                item.mapping_candidates = candidates.clone();
                if let Some(best) = unique_exact_candidate(&candidates) {
                    let mapping = SavedMapping {
                        app_id: best.app_id,
                        steam_name: best.name.clone(),
                        source: "automatic".to_string(),
                    };
                    apply_mapping(item, &mapping);
                    mappings.records.insert(item.mapping_key.clone(), mapping);
                }
            }
            view.entitlements.summary = crate::entitlements::summarise(&view.entitlements.items);
        })
        .await;
    save_mapping_file(&mappings).await
}

async fn apply_store_exact_mappings(state: &AppState) -> Result<(), String> {
    let unresolved = {
        let view = state.view.read().await;
        view.entitlements
            .items
            .iter()
            .filter(|item| item.status == "needs_mapping")
            .map(|item| (item.mapping_key.clone(), item.name.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    if unresolved.is_empty() {
        return Ok(());
    }

    let now = unix_time();
    let total = unresolved.len();
    let mut cache = load_store_search_file().await?;
    let mut results = BTreeMap::new();
    for (index, (mapping_key, title)) in unresolved.into_iter().enumerate() {
        let cached = cache
            .records
            .get(&mapping_key)
            .filter(|record| {
                record.query == title
                    && now.saturating_sub(record.checked_at) < STORE_SEARCH_CACHE_TTL_SECONDS
            })
            .cloned();
        let candidates = if let Some(record) = cached {
            record.candidates
        } else if (2..=120).contains(&title.chars().count()) {
            match search_store(&title).await {
                Ok(candidates) => {
                    cache.records.insert(
                        mapping_key.clone(),
                        StoreSearchRecord {
                            query: title,
                            checked_at: now,
                            candidates: candidates.clone(),
                        },
                    );
                    candidates
                }
                Err(_) => continue,
            }
        } else {
            continue;
        };
        results.insert(mapping_key, candidates);
        state
            .update_view(|view| {
                view.wishlists.message = format!(
                    "Checking unresolved titles on Steam ({}/{total})…",
                    index + 1
                );
            })
            .await;
    }
    save_store_search_file(&cache).await?;

    let mut mappings = load_mapping_file().await?;
    state
        .update_view(|view| {
            for item in &mut view.entitlements.items {
                if item.status != "needs_mapping" {
                    continue;
                }
                let Some(candidates) = results.get(&item.mapping_key) else {
                    continue;
                };
                merge_mapping_candidates(&mut item.mapping_candidates, candidates);
                if let Some(candidate) = unique_exact_candidate(candidates) {
                    let mapping = SavedMapping {
                        app_id: candidate.app_id,
                        steam_name: candidate.name.clone(),
                        source: "automatic".to_string(),
                    };
                    apply_mapping(item, &mapping);
                    mappings.records.insert(item.mapping_key.clone(), mapping);
                }
            }
            view.entitlements.summary = crate::entitlements::summarise(&view.entitlements.items);
        })
        .await;
    save_mapping_file(&mappings).await
}

fn unique_exact_candidate(candidates: &[MappingCandidateView]) -> Option<&MappingCandidateView> {
    let mut exact = candidates
        .iter()
        .filter(|candidate| candidate.similarity >= 0.999);
    let candidate = exact.next()?;
    exact
        .all(|other| other.app_id == candidate.app_id)
        .then_some(candidate)
}

fn merge_mapping_candidates(
    existing: &mut Vec<MappingCandidateView>,
    additional: &[MappingCandidateView],
) {
    for candidate in additional {
        if let Some(current) = existing
            .iter_mut()
            .find(|current| current.app_id == candidate.app_id)
        {
            if candidate.similarity > current.similarity {
                *current = candidate.clone();
            }
        } else {
            existing.push(candidate.clone());
        }
    }
    existing.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
    existing.truncate(12);
}

fn apply_mapping(item: &mut crate::state::EntitlementView, mapping: &SavedMapping) {
    item.steam_app_id = Some(mapping.app_id);
    item.steam_name = Some(mapping.steam_name.clone());
    item.mapping_source = Some(mapping.source.clone());
    item.status = "available".to_string();
    item.reasons
        .retain(|reason| !reason.contains("did not provide a valid Steam AppID"));
    if !item
        .reasons
        .iter()
        .any(|reason| reason.contains("title match") || reason.contains("corrected locally"))
    {
        item.reasons.push(if mapping.source == "automatic" {
            format!(
                "Automatic high-confidence Steam title match: {}.",
                mapping.steam_name
            )
        } else {
            format!("Steam mapping corrected locally: {}.", mapping.steam_name)
        });
    }
}

fn has_local_mapping(item: &crate::state::EntitlementView) -> bool {
    matches!(item.mapping_source.as_deref(), Some("manual" | "automatic"))
}

fn rank_candidates(title: &str, identities: &[AppIdentity]) -> Vec<MappingCandidateView> {
    let mut candidates = identities
        .iter()
        .map(|identity| MappingCandidateView {
            app_id: identity.app_id,
            name: identity.name.clone(),
            similarity: title_similarity(title, &identity.name),
        })
        .filter(|candidate| candidate.similarity >= 0.60)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
    candidates.truncate(5);
    candidates
}

fn title_similarity(left: &str, right: &str) -> f64 {
    let left_tokens = normalise_title_tokens(left);
    let right_tokens = normalise_title_tokens(right);
    let left_numbers = numeric_tokens(&left_tokens);
    let right_numbers = numeric_tokens(&right_tokens);
    if left_numbers != right_numbers {
        return 0.0;
    }
    let left = left_tokens.concat();
    let right = right_tokens.concat();
    if left == right {
        return 1.0;
    }
    let distance = levenshtein(&left, &right);
    1.0 - distance as f64 / left.chars().count().max(right.chars().count()).max(1) as f64
}

fn normalise_title_tokens(value: &str) -> Vec<String> {
    let without_store_suffix = value
        .strip_suffix("(Steam)")
        .or_else(|| value.strip_suffix("(steam)"))
        .unwrap_or(value);
    let mut tokens = without_store_suffix
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .map(|token| match token.as_str() {
            "i" => "1".to_string(),
            "ii" => "2".to_string(),
            "iii" => "3".to_string(),
            "iv" => "4".to_string(),
            "v" => "5".to_string(),
            "vi" => "6".to_string(),
            "games" => "game".to_string(),
            _ => token,
        })
        .collect::<Vec<_>>();
    if tokens.ends_with(&["definitive".to_string(), "edition".to_string()]) {
        tokens.truncate(tokens.len() - 2);
    }
    tokens
}

fn numeric_tokens(tokens: &[String]) -> Vec<&str> {
    tokens
        .iter()
        .filter(|token| token.chars().all(|character| character.is_ascii_digit()))
        .map(String::as_str)
        .collect()
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + usize::from(left_character != *right_character)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

pub async fn rebuild_matches(state: &AppState) {
    let memberships = state.wishlist_memberships.read().await.clone();
    state
        .update_view(|view| {
            let mut matches = view
                .entitlements
                .items
                .iter()
                .filter(|item| item.status == "available")
                .filter_map(|item| {
                    let app_id = item.steam_app_id?;
                    let wishers = memberships.get(&app_id)?.clone();
                    Some(GiftMatchView {
                        entitlement_id: item.id.clone(),
                        app_id,
                        steam_name: item.steam_name.clone().unwrap_or_else(|| item.name.clone()),
                        wishers,
                    })
                })
                .collect::<Vec<_>>();
            matches.sort_by(|left, right| {
                right.wishers.len().cmp(&left.wishers.len()).then_with(|| {
                    left.steam_name
                        .to_lowercase()
                        .cmp(&right.steam_name.to_lowercase())
                })
            });
            view.wishlists.matches = matches;
        })
        .await;
}

async fn load_mapping_file() -> Result<MappingFile, String> {
    read_json_file(mapping_path()?)
        .await
        .map(migrate_mapping_file)
}

fn migrate_mapping_file(mut file: MappingFile) -> MappingFile {
    if file.version == MAPPING_CACHE_VERSION {
        return file;
    }
    file.records.retain(|_, mapping| mapping.source == "manual");
    MappingFile {
        version: MAPPING_CACHE_VERSION,
        records: file.records,
    }
}

async fn save_mapping_file(file: &MappingFile) -> Result<(), String> {
    write_json_file(mapping_path()?, file).await
}

async fn load_identity_file() -> Result<IdentityFile, String> {
    read_json_file(identity_path()?)
        .await
        .map(|file: IdentityFile| {
            if file.version == CACHE_VERSION {
                file
            } else {
                IdentityFile {
                    version: CACHE_VERSION,
                    ..Default::default()
                }
            }
        })
}

async fn save_identity_file(file: &IdentityFile) -> Result<(), String> {
    write_json_file(identity_path()?, file).await
}

async fn load_store_search_file() -> Result<StoreSearchFile, String> {
    read_json_file(store_search_path()?)
        .await
        .map(|file: StoreSearchFile| {
            if file.version == STORE_SEARCH_CACHE_VERSION {
                file
            } else {
                StoreSearchFile {
                    version: STORE_SEARCH_CACHE_VERSION,
                    ..Default::default()
                }
            }
        })
}

async fn save_store_search_file(file: &StoreSearchFile) -> Result<(), String> {
    write_json_file(store_search_path()?, file).await
}

async fn read_json_file<T: for<'de> Deserialize<'de> + Default + Send + 'static>(
    path: PathBuf,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.to_string()),
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn write_json_file<T: Serialize>(path: PathBuf, value: &T) -> Result<(), String> {
    let encoded = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let parent = path
            .parent()
            .ok_or_else(|| "Cache path has no parent.".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
        }
        file.write_all(&encoded).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn mapping_path() -> Result<PathBuf, String> {
    app_data_path("mappings-v1.json")
}

fn identity_path() -> Result<PathBuf, String> {
    app_data_path("steam-app-identities-v1.json")
}

fn store_search_path() -> Result<PathBuf, String> {
    app_data_path("steam-title-searches-v1.json")
}

fn app_data_path(filename: &str) -> Result<PathBuf, String> {
    dirs_next::data_local_dir()
        .map(|directory| directory.join("Humble Gift Matcher").join(filename))
        .ok_or_else(|| "Could not locate the local application-data directory.".to_string())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitise_error(error: &str) -> String {
    error
        .lines()
        .next()
        .unwrap_or("Unknown Steam wishlist error")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(app_id: u32, name: &str) -> AppIdentity {
        AppIdentity {
            app_id,
            name: name.to_string(),
            app_type: "game".to_string(),
        }
    }

    #[test]
    fn punctuation_trademarks_and_roman_numerals_normalise() {
        assert_eq!(
            title_similarity("BATTLETECH - Flashpoint", "BATTLETECH Flashpoint"),
            1.0
        );
        assert_eq!(
            title_similarity(
                "The Elder Scrolls® V: Skyrim® Special Edition",
                "The Elder Scrolls V: Skyrim Special Edition"
            ),
            1.0
        );
        assert_eq!(
            title_similarity(
                "Cities: Skylines - Green Cities (Steam)",
                "Cities: Skylines - Green Cities"
            ),
            1.0
        );
        assert_eq!(
            title_similarity("Gamedec", "Gamedec - Definitive Edition"),
            1.0
        );
        assert_eq!(
            title_similarity(
                "The Lord of the Rings: Adventure Card Games",
                "The Lord of the Rings: Adventure Card Game - Definitive Edition"
            ),
            1.0
        );
    }

    #[test]
    fn humble_app_ids_are_rejected_when_steam_identifies_another_product() {
        assert!(humble_mapping_mismatch(
            "Gamedec",
            &AppIdentity {
                app_id: 295047,
                name: "Rocksmith® 2014 – Tom Petty and the Heartbreakers - “American Girl”"
                    .to_string(),
                app_type: "dlc".to_string(),
            }
        ));
        assert!(humble_mapping_mismatch(
            "Blade Assault",
            &identity(1307580, "TOEM: A Photo Adventure")
        ));
        assert!(humble_mapping_mismatch(
            "The Lord of the Rings: Adventure Card Games",
            &AppIdentity {
                app_id: 862380,
                name: "The Lord of the Rings: Adventure Card Game Soundtrack".to_string(),
                app_type: "music".to_string(),
            }
        ));
        assert!(!humble_mapping_mismatch(
            "Gamedec",
            &identity(917720, "Gamedec - Definitive Edition")
        ));
        assert!(!humble_mapping_mismatch(
            "Example Soundtrack",
            &AppIdentity {
                app_id: 1,
                name: "Example Soundtrack".to_string(),
                app_type: "music".to_string(),
            }
        ));
    }

    #[test]
    fn store_search_retries_with_a_canonical_punctuation_free_title() {
        assert_eq!(
            store_search_terms("BATTLETECH - Flashpoint"),
            vec![
                "BATTLETECH - Flashpoint".to_string(),
                "battletech flashpoint".to_string()
            ]
        );
        assert_eq!(
            store_search_terms("BATTLETECH"),
            vec!["BATTLETECH".to_string()]
        );
    }

    #[test]
    fn complete_wishlists_are_grouped_and_sorted_for_each_person() {
        let alice = WishlistPersonView {
            steam_id: "alice".to_string(),
            display_name: "Alice".to_string(),
            avatar_url: None,
            is_self: false,
            wishlist_access: "accessible".to_string(),
            wishlist_count: 2,
        };
        let private = WishlistPersonView {
            steam_id: "private".to_string(),
            display_name: "Private".to_string(),
            avatar_url: None,
            is_self: false,
            wishlist_access: "inaccessible".to_string(),
            wishlist_count: 0,
        };
        let memberships = BTreeMap::from([(20, vec![alice.clone()]), (10, vec![alice.clone()])]);
        let wishlists = build_person_wishlists(
            &[alice, private],
            &memberships,
            &[identity(10, "Zeta"), identity(20, "Alpha")],
        );

        assert_eq!(
            wishlists["alice"]
                .iter()
                .map(|game| game.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zeta"]
        );
        assert!(!wishlists.contains_key("private"));
    }

    #[test]
    fn numbered_sequels_do_not_match() {
        assert_eq!(
            title_similarity("Cities: Skylines", "Cities: Skylines II"),
            0.0
        );
    }

    #[test]
    fn ranking_keeps_likely_candidates_and_orders_them() {
        let candidates = rank_candidates(
            "Ultimate General Gettysburg",
            &[
                identity(1, "Ultimate General: Gettysburg"),
                identity(2, "Ultimate General: Civil War"),
            ],
        );
        assert_eq!(candidates[0].app_id, 1);
        assert_eq!(candidates[0].similarity, 1.0);
    }

    #[test]
    fn store_search_results_become_mapping_candidates() {
        let response = serde_json::json!({
            "items": [
                { "type": "app", "name": "Ambiguous Quest", "id": 456 },
                { "type": "app", "name": "", "id": 999 },
                { "type": "app", "name": "Invalid", "id": 0 }
            ]
        });
        let results = parse_store_search(&response, "Ambiguous Quest Deluxe");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].app_id, 456);
        assert_eq!(results[0].name, "Ambiguous Quest");
        assert!(results[0].similarity > 0.6);
    }

    #[test]
    fn only_unique_exact_store_matches_are_automatic() {
        let one_exact = vec![
            MappingCandidateView {
                app_id: 10,
                name: "Exact Game".to_string(),
                similarity: 1.0,
            },
            MappingCandidateView {
                app_id: 11,
                name: "Exact Game Deluxe".to_string(),
                similarity: 0.8,
            },
        ];
        assert_eq!(unique_exact_candidate(&one_exact).unwrap().app_id, 10);

        let ambiguous = vec![
            MappingCandidateView {
                app_id: 10,
                name: "Exact Game".to_string(),
                similarity: 1.0,
            },
            MappingCandidateView {
                app_id: 12,
                name: "Exact Game".to_string(),
                similarity: 1.0,
            },
        ];
        assert!(unique_exact_candidate(&ambiguous).is_none());
    }

    #[test]
    fn mapping_upgrade_preserves_manual_corrections_only() {
        let mut records = BTreeMap::new();
        records.insert(
            "manual".to_string(),
            SavedMapping {
                app_id: 1,
                steam_name: "Manual".to_string(),
                source: "manual".to_string(),
            },
        );
        records.insert(
            "old-auto".to_string(),
            SavedMapping {
                app_id: 2,
                steam_name: "Old automatic".to_string(),
                source: "automatic".to_string(),
            },
        );
        let migrated = migrate_mapping_file(MappingFile {
            version: 1,
            records,
        });
        assert_eq!(migrated.version, MAPPING_CACHE_VERSION);
        assert!(migrated.records.contains_key("manual"));
        assert!(!migrated.records.contains_key("old-auto"));
    }
}
