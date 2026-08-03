use crate::state::{AppState, SteamReviewSummaryView};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

const REVIEWS_URL: &str = "https://store.steampowered.com/appreviews";
const CACHE_VERSION: u32 = 1;
const CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const REQUEST_CONCURRENCY: usize = 4;

#[derive(Clone, Deserialize, Serialize)]
struct CachedReviewSummary {
    checked_at: u64,
    summary: SteamReviewSummaryView,
}

#[derive(Default, Deserialize, Serialize)]
struct ReviewCache {
    version: u32,
    records: BTreeMap<u32, CachedReviewSummary>,
}

#[tauri::command]
pub async fn load_steam_reviews(state: State<'_, AppState>) -> Result<(), String> {
    start_load(state.inner().clone()).await
}

async fn start_load(state: AppState) -> Result<(), String> {
    if state.view.read().await.steam_reviews.phase == "loading" {
        return Ok(());
    }
    let app_ids = {
        let view = state.view.read().await;
        view.entitlements
            .items
            .iter()
            .filter(|item| item.status == "available")
            .filter_map(|item| item.steam_app_id)
            .collect::<BTreeSet<_>>()
    };
    let total = app_ids.len();
    state
        .update_view(|view| {
            view.steam_reviews.phase = "loading".to_string();
            view.steam_reviews.message = format!("Loading Steam ratings for {total} games…");
            view.steam_reviews.error = None;
            view.steam_reviews.completed = 0;
            view.steam_reviews.total = total;
        })
        .await;

    if let Some(task) = state.steam_review_task.lock().await.take() {
        task.abort();
    }
    let task_state = state.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(error) = run_load(&task_state, app_ids).await {
            task_state
                .update_view(|view| {
                    view.steam_reviews.phase = "error".to_string();
                    view.steam_reviews.message = "Steam rating loading stopped.".to_string();
                    view.steam_reviews.error = Some(sanitise_error(&error));
                })
                .await;
        }
    });
    *state.steam_review_task.lock().await = Some(task);
    Ok(())
}

async fn run_load(state: &AppState, app_ids: BTreeSet<u32>) -> Result<(), String> {
    let now = unix_time();
    let mut cache = load_cache().await?;
    if cache.version != CACHE_VERSION {
        cache = ReviewCache {
            version: CACHE_VERSION,
            ..Default::default()
        };
    }
    let mut summaries = BTreeMap::new();
    let mut missing = Vec::new();
    for app_id in app_ids {
        match cache.records.get(&app_id).filter(|record| {
            now.saturating_sub(record.checked_at) < CACHE_TTL_SECONDS
                && record.checked_at <= now.saturating_add(5 * 60)
        }) {
            Some(record) => {
                summaries.insert(app_id, record.summary.clone());
            }
            None => missing.push(app_id),
        }
    }
    let cached_count = summaries.len();
    state
        .update_view(|view| {
            view.steam_reviews.items = summaries;
            view.steam_reviews.completed = cached_count;
            view.steam_reviews.message = if missing.is_empty() {
                "Steam ratings loaded from cache.".to_string()
            } else {
                format!("{cached_count} ratings cached · {} to fetch", missing.len())
            };
        })
        .await;

    if missing.is_empty() {
        finish_load(state, 0).await;
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent("Humble Gift Matcher/1.0")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| error.to_string())?;
    let mut requests = stream::iter(missing.into_iter().map(|app_id| {
        let client = client.clone();
        async move { (app_id, fetch_review_summary(&client, app_id).await) }
    }))
    .buffer_unordered(REQUEST_CONCURRENCY);
    let mut completed = cached_count;
    let mut failures = 0;
    while let Some((app_id, result)) = requests.next().await {
        completed += 1;
        match result {
            Ok(summary) => {
                cache.records.insert(
                    app_id,
                    CachedReviewSummary {
                        checked_at: now,
                        summary: summary.clone(),
                    },
                );
                state
                    .update_view(|view| {
                        view.steam_reviews.items.insert(app_id, summary);
                        view.steam_reviews.completed = completed;
                        view.steam_reviews.message = format!(
                            "Loaded {completed} of {} Steam ratings…",
                            view.steam_reviews.total
                        );
                    })
                    .await;
            }
            Err(_) => {
                failures += 1;
                state
                    .update_view(|view| {
                        view.steam_reviews.completed = completed;
                        view.steam_reviews.message = format!(
                            "Loaded {completed} of {} Steam ratings…",
                            view.steam_reviews.total
                        );
                    })
                    .await;
            }
        }
        if completed % 25 == 0 {
            save_cache(&cache).await?;
        }
    }
    save_cache(&cache).await?;
    finish_load(state, failures).await;
    Ok(())
}

async fn finish_load(state: &AppState, failures: usize) {
    state
        .update_view(|view| {
            view.steam_reviews.phase = "complete".to_string();
            view.steam_reviews.message = format!(
                "{} Steam ratings available.",
                view.steam_reviews.items.len()
            );
            view.steam_reviews.error = (failures > 0).then(|| {
                format!(
                    "{failures} {} could not be rated by Steam.",
                    if failures == 1 { "game" } else { "games" }
                )
            });
        })
        .await;
}

pub(crate) async fn fetch_review_summary(
    client: &reqwest::Client,
    app_id: u32,
) -> Result<SteamReviewSummaryView, String> {
    let response = client
        .get(format!("{REVIEWS_URL}/{app_id}"))
        .query(&[
            ("json", "1"),
            ("filter", "all"),
            ("language", "all"),
            ("purchase_type", "steam"),
            ("num_per_page", "1"),
        ])
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Steam returned HTTP {}.", response.status()));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| "Steam returned invalid review data.".to_string())?;
    parse_review_summary(app_id, &value)
}

fn parse_review_summary(app_id: u32, value: &Value) -> Result<SteamReviewSummaryView, String> {
    if value.get("success").and_then(Value::as_u64) != Some(1) {
        return Err("Steam did not return a review summary.".to_string());
    }
    let summary = value
        .get("query_summary")
        .and_then(Value::as_object)
        .ok_or_else(|| "Steam omitted the review summary.".to_string())?;
    let total_positive = summary
        .get("total_positive")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_negative = summary
        .get("total_negative")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_reviews = summary
        .get("total_reviews")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| total_positive.saturating_add(total_negative));
    let positive_percentage =
        (total_reviews > 0).then(|| total_positive as f64 / total_reviews as f64 * 100.0);
    Ok(SteamReviewSummaryView {
        app_id,
        positive_percentage,
        total_positive,
        total_negative,
        total_reviews,
        score_description: summary
            .get("review_score_desc")
            .and_then(Value::as_str)
            .unwrap_or(if total_reviews > 0 {
                "Steam reviews"
            } else {
                "No user reviews"
            })
            .to_string(),
    })
}

async fn load_cache() -> Result<ReviewCache, String> {
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "The Steam review cache is unreadable.".to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ReviewCache {
            version: CACHE_VERSION,
            ..Default::default()
        }),
        Err(error) => Err(format!("Could not read the Steam review cache: {error}")),
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn save_cache(cache: &ReviewCache) -> Result<(), String> {
    let encoded = serde_json::to_vec(cache).map_err(|error| error.to_string())?;
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || {
        let parent = path
            .parent()
            .ok_or_else(|| "Steam review cache path has no parent.".to_string())?;
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

fn cache_path() -> Result<PathBuf, String> {
    dirs_next::data_local_dir()
        .map(|directory| {
            directory
                .join("Humble Gift Matcher")
                .join("steam-review-summaries-v1.json")
        })
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
        .unwrap_or("Unknown Steam review error")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_summary_uses_lifetime_positive_percentage() {
        let value = serde_json::json!({
            "success": 1,
            "query_summary": {
                "review_score_desc": "Very Positive",
                "total_positive": 950,
                "total_negative": 50,
                "total_reviews": 1000
            }
        });
        let summary = parse_review_summary(123, &value).unwrap();

        assert_eq!(summary.app_id, 123);
        assert_eq!(summary.positive_percentage, Some(95.0));
        assert_eq!(summary.total_reviews, 1000);
        assert_eq!(summary.score_description, "Very Positive");
    }

    #[test]
    fn games_without_reviews_are_retained_but_unrated() {
        let value = serde_json::json!({
            "success": 1,
            "query_summary": {
                "review_score_desc": "No user reviews",
                "total_positive": 0,
                "total_negative": 0,
                "total_reviews": 0
            }
        });
        let summary = parse_review_summary(456, &value).unwrap();

        assert_eq!(summary.positive_percentage, None);
        assert_eq!(summary.total_reviews, 0);
    }
}
