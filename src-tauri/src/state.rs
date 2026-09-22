use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use steamroom::client::{LoggedIn, SteamClient};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamProfile {
    pub steam_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamConnectionView {
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
    pub remembered: bool,
    pub qr_image: Option<String>,
    pub profile: Option<SteamProfile>,
    pub ownership_loaded: bool,
    pub owned_app_ids: BTreeSet<u32>,
}

impl Default for SteamConnectionView {
    fn default() -> Self {
        Self {
            phase: "disconnected".to_string(),
            message: String::new(),
            error: None,
            remembered: false,
            qr_image: None,
            profile: None,
            ownership_loaded: false,
            owned_app_ids: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumbleConnectionView {
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
    pub remembered: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementSummary {
    pub total: usize,
    pub available: usize,
    pub needs_mapping: usize,
    pub revealed: usize,
    pub excluded: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementView {
    pub id: String,
    pub mapping_key: String,
    pub name: String,
    pub parent_name: String,
    pub steam_app_id: Option<u32>,
    pub steam_name: Option<String>,
    pub mapping_source: Option<String>,
    pub mapping_candidates: Vec<MappingCandidateView>,
    pub key_type_label: String,
    pub status: String,
    pub reasons: Vec<String>,
    pub purchase_url: Option<String>,
    #[serde(default)]
    pub expiration_date: Option<String>,
    pub region_restricted: bool,
    pub package_ambiguity: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingCandidateView {
    pub app_id: u32,
    pub name: String,
    pub similarity: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistPersonView {
    pub steam_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub is_self: bool,
    pub wishlist_access: String,
    pub wishlist_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistGameView {
    pub app_id: u32,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamReviewSummaryView {
    pub app_id: u32,
    pub positive_percentage: Option<f64>,
    pub total_positive: u64,
    pub total_negative: u64,
    pub total_reviews: u64,
    pub score_description: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamReviewSyncView {
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
    pub completed: usize,
    pub total: usize,
    pub items: BTreeMap<u32, SteamReviewSummaryView>,
}

impl Default for SteamReviewSyncView {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            message: "Steam ratings load when you browse entitlements.".to_string(),
            error: None,
            completed: 0,
            total: 0,
            items: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiftMatchView {
    pub entitlement_id: String,
    pub app_id: u32,
    pub steam_name: String,
    pub wishers: Vec<WishlistPersonView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistSyncView {
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
    pub people_total: usize,
    pub people_accessible: usize,
    pub people_inaccessible: usize,
    pub wishlist_apps: usize,
    pub matches: Vec<GiftMatchView>,
    pub people: Vec<WishlistPersonView>,
    pub wishlist_items: BTreeMap<String, Vec<WishlistGameView>>,
}

impl Default for WishlistSyncView {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            message: "Ready to load accessible Steam wishlists.".to_string(),
            error: None,
            people_total: 0,
            people_accessible: 0,
            people_inaccessible: 0,
            wishlist_apps: 0,
            matches: Vec::new(),
            people: Vec::new(),
            wishlist_items: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementSyncView {
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
    pub completed_orders: usize,
    pub total_orders: usize,
    pub refreshed_at: Option<u64>,
    pub summary: EntitlementSummary,
    pub items: Vec<EntitlementView>,
}

impl Default for EntitlementSyncView {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            message: "Ready to load available Humble entitlements.".to_string(),
            error: None,
            completed_orders: 0,
            total_orders: 0,
            refreshed_at: None,
            summary: EntitlementSummary::default(),
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub steam: SteamConnectionView,
    pub humble: HumbleConnectionView,
    pub entitlements: EntitlementSyncView,
    pub wishlists: WishlistSyncView,
    pub steam_reviews: SteamReviewSyncView,
    pub development_cache: bool,
}

impl Default for AppView {
    fn default() -> Self {
        Self {
            steam: SteamConnectionView {
                phase: "disconnected".to_string(),
                message: "Connect Steam to read your identity and accessible wishlists."
                    .to_string(),
                error: None,
                remembered: false,
                qr_image: None,
                profile: None,
                ownership_loaded: false,
                owned_app_ids: BTreeSet::new(),
            },
            humble: HumbleConnectionView {
                phase: "disconnected".to_string(),
                message: "Sign in on Humble's own page to load available entitlements.".to_string(),
                error: None,
                remembered: false,
            },
            entitlements: EntitlementSyncView::default(),
            wishlists: WishlistSyncView::default(),
            steam_reviews: SteamReviewSyncView::default(),
            development_cache: crate::credential_store::development_cache_enabled(),
        }
    }
}

#[allow(dead_code)]
pub struct SteamSession {
    pub client: SteamClient<LoggedIn>,
    pub steam_id: u64,
    pub friends: Vec<SteamProfile>,
}

#[derive(Clone, Default)]
pub struct AppState {
    pub view: Arc<RwLock<AppView>>,
    pub steam_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub humble_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub wishlist_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub steam_review_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub steam_protocol_gate: Arc<Mutex<()>>,
    pub wishlist_memberships: Arc<RwLock<BTreeMap<u32, Vec<WishlistPersonView>>>>,
    pub steam_session: Arc<RwLock<Option<SteamSession>>>,
    pub humble_session: Arc<RwLock<Option<String>>>,
    #[cfg(debug_assertions)]
    pub humble_cookie_signature: Arc<Mutex<Option<String>>>,
    pub humble_rejected_session_signature: Arc<Mutex<Option<u64>>>,
    pub humble_validation_gate: Arc<Mutex<()>>,
}

impl AppState {
    pub async fn update_view(&self, update: impl FnOnce(&mut AppView)) {
        let mut view = self.view.write().await;
        update(&mut view);
    }
}
