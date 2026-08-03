use crate::reviews;
use crate::state::{AppState, SteamReviewSummaryView};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State, Url};
use tauri_plugin_opener::OpenerExt;

const APP_DETAILS_URL: &str = "https://store.steampowered.com/api/appdetails";
const CACHE_VERSION: u32 = 1;
const CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGameDetailsView {
    pub app_id: u32,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub genres: Vec<String>,
    pub features: Vec<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub release_date: Option<String>,
    pub header_image: Option<String>,
    pub screenshots: Vec<String>,
    pub review: Option<SteamReviewSummaryView>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CachedGameDetails {
    checked_at: u64,
    details: SteamGameDetailsView,
}

#[derive(Default, Deserialize, Serialize)]
struct GameDetailsCache {
    version: u32,
    records: BTreeMap<u32, CachedGameDetails>,
}

#[tauri::command]
pub async fn load_steam_game_details(
    app_id: u32,
    state: State<'_, AppState>,
) -> Result<SteamGameDetailsView, String> {
    if app_id == 0 {
        return Err("Enter a valid Steam AppID.".to_string());
    }

    let now = unix_time();
    let mut cache = load_cache().await?;
    if cache.version != CACHE_VERSION {
        cache = GameDetailsCache {
            version: CACHE_VERSION,
            ..Default::default()
        };
    }
    if let Some(record) = cache.records.get(&app_id).filter(|record| {
        record.checked_at <= now.saturating_add(5 * 60)
            && now.saturating_sub(record.checked_at) < CACHE_TTL_SECONDS
    }) {
        let mut details = record.details.clone();
        if let Some(review) = state
            .view
            .read()
            .await
            .steam_reviews
            .items
            .get(&app_id)
            .cloned()
        {
            details.review = Some(review);
        }
        return Ok(details);
    }

    let client = reqwest::Client::builder()
        .user_agent("Humble Gift Matcher/0.1")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let mut details = fetch_game_details(&client, app_id).await?;
    details.review = if let Some(review) = state
        .view
        .read()
        .await
        .steam_reviews
        .items
        .get(&app_id)
        .cloned()
    {
        Some(review)
    } else {
        reviews::fetch_review_summary(&client, app_id).await.ok()
    };
    if let Some(review) = details.review.clone() {
        state
            .update_view(|view| {
                view.steam_reviews.items.insert(app_id, review);
            })
            .await;
    }
    cache.records.insert(
        app_id,
        CachedGameDetails {
            checked_at: now,
            details: details.clone(),
        },
    );
    save_cache(&cache).await?;
    Ok(details)
}

#[tauri::command]
pub fn open_steam_game(app: AppHandle, app_id: u32) -> Result<(), String> {
    if app_id == 0 {
        return Err("Enter a valid Steam AppID.".to_string());
    }
    app.opener()
        .open_url(format!("steam://store/{app_id}"), None::<&str>)
        .map_err(|error| error.to_string())
}

async fn fetch_game_details(
    client: &reqwest::Client,
    app_id: u32,
) -> Result<SteamGameDetailsView, String> {
    let response = client
        .get(APP_DETAILS_URL)
        .query(&[
            ("appids", app_id.to_string()),
            ("l", "english".to_string()),
            ("cc", "GB".to_string()),
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
        .map_err(|_| "Steam returned invalid game details.".to_string())?;
    parse_game_details(app_id, &value)
}

fn parse_game_details(app_id: u32, value: &Value) -> Result<SteamGameDetailsView, String> {
    let key = app_id.to_string();
    let app = value
        .get(&key)
        .and_then(Value::as_object)
        .ok_or_else(|| "Steam omitted this game.".to_string())?;
    if app.get("success").and_then(Value::as_bool) != Some(true) {
        return Err("Steam did not return this game's store listing.".to_string());
    }
    let data = app
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "Steam omitted this game's store listing.".to_string())?;
    let name = plain_text(
        string_field(data, "name").ok_or_else(|| "Steam omitted this game's title.".to_string())?,
    );
    let summary = plain_text(string_field(data, "short_description").unwrap_or(""));
    let description = plain_text(
        string_field(data, "about_the_game")
            .or_else(|| string_field(data, "detailed_description"))
            .unwrap_or(""),
    );

    Ok(SteamGameDetailsView {
        app_id,
        name,
        summary,
        description,
        genres: description_list(data, "genres"),
        features: description_list(data, "categories"),
        developers: string_list(data, "developers"),
        publishers: string_list(data, "publishers"),
        release_date: data
            .get("release_date")
            .and_then(Value::as_object)
            .and_then(|release| release.get("date"))
            .and_then(Value::as_str)
            .filter(|date| !date.trim().is_empty())
            .map(str::to_string),
        header_image: string_field(data, "header_image").and_then(steam_image_url),
        screenshots: data
            .get("screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|screenshot| {
                screenshot
                    .get("path_full")
                    .and_then(Value::as_str)
                    .and_then(steam_image_url)
            })
            .take(6)
            .collect(),
        review: None,
    })
}

fn string_field<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn string_list(data: &Map<String, Value>, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(8)
        .map(str::to_string)
        .collect()
}

fn description_list(data: &Map<String, Value>, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("description").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(10)
        .map(str::to_string)
        .collect()
}

fn steam_image_url(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host == "steamstatic.com" || host.ends_with(".steamstatic.com")))
    .then(|| url.to_string())
}

fn plain_text(value: &str) -> String {
    let mut text = String::with_capacity(value.len().min(6_000));
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => {
                in_tag = true;
                if !text.ends_with(char::is_whitespace) {
                    text.push(' ');
                }
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
        if text.chars().count() >= 6_000 {
            break;
        }
    }
    let decoded = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn load_cache() -> Result<GameDetailsCache, String> {
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "The Steam game-details cache is unreadable.".to_string()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(GameDetailsCache {
            version: CACHE_VERSION,
            ..Default::default()
        }),
        Err(error) => Err(format!(
            "Could not read the Steam game-details cache: {error}"
        )),
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn save_cache(cache: &GameDetailsCache) -> Result<(), String> {
    let encoded = serde_json::to_vec(cache).map_err(|error| error.to_string())?;
    let path = cache_path()?;
    tauri::async_runtime::spawn_blocking(move || {
        let parent = path
            .parent()
            .ok_or_else(|| "Steam game-details cache path has no parent.".to_string())?;
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
                .join("steam-game-details-v1.json")
        })
        .ok_or_else(|| "Could not locate the local application-data directory.".to_string())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_safe_store_listing_details() {
        let value = serde_json::json!({
            "123": {
                "success": true,
                "data": {
                    "name": "Example &amp; Friends",
                    "short_description": "A <b>small</b> adventure.",
                    "about_the_game": "<h2>Explore</h2><p>Find &amp; share.</p>",
                    "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/example.jpg",
                    "developers": ["Small Studio"],
                    "publishers": ["Good Publisher"],
                    "genres": [{"description": "Adventure"}],
                    "categories": [{"description": "Single-player"}],
                    "release_date": {"date": "1 Jan, 2026"},
                    "screenshots": [
                        {"path_full": "https://shared.akamai.steamstatic.com/store_item_assets/one.jpg"},
                        {"path_full": "https://example.com/not-steam.jpg"}
                    ]
                }
            }
        });
        let details = parse_game_details(123, &value).unwrap();

        assert_eq!(details.name, "Example & Friends");
        assert_eq!(details.summary, "A small adventure.");
        assert_eq!(details.description, "Explore Find & share.");
        assert_eq!(details.genres, vec!["Adventure"]);
        assert_eq!(details.features, vec!["Single-player"]);
        assert_eq!(details.screenshots.len(), 1);
    }

    #[test]
    fn rejects_non_steam_image_hosts() {
        assert!(steam_image_url("https://example.com/image.jpg").is_none());
        assert!(steam_image_url("http://cdn.steamstatic.com/image.jpg").is_none());
        assert!(steam_image_url("https://steamstatic.com.evil.test/image.jpg").is_none());
    }
}
