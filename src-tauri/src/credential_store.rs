use serde::{Deserialize, Serialize};

const DEVELOPMENT_CACHE_ENV: &str = "HGM_INSECURE_DEV_CREDENTIAL_CACHE";
const KEYRING_SERVICE: &str = "com.jakemartin.humble-gift-matcher";
#[cfg(target_os = "macos")]
const CREDENTIALS_ENTRY: &str = "credentials-v1";
#[cfg(not(target_os = "macos"))]
const STEAM_LOGIN_ENTRY: &str = "steam-login";
#[cfg(not(target_os = "macos"))]
const STEAM_ACCESS_TOKEN_ENTRY: &str = "steam-access-token";
#[cfg(not(target_os = "macos"))]
const HUMBLE_SESSION_ENTRY: &str = "humble-session";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SavedSteamLogin {
    pub account_name: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevelopmentCredentials {
    pub steam: Option<SavedSteamLogin>,
    pub humble_session: Option<String>,
}

pub fn development_cache_enabled() -> bool {
    cfg!(debug_assertions)
        && std::env::var(DEVELOPMENT_CACHE_ENV)
            .ok()
            .is_some_and(|value| development_cache_value_enabled(&value))
}

pub async fn load() -> Result<DevelopmentCredentials, String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::load().await;
    }

    let credentials = secure::load().await?;

    #[cfg(debug_assertions)]
    {
        migrate_development_cache(credentials).await
    }

    #[cfg(not(debug_assertions))]
    {
        Ok(credentials)
    }
}

pub async fn save_steam(login: SavedSteamLogin) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::save_steam(login).await;
    }

    secure::save_steam(login).await
}

pub async fn delete_steam() -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::delete_steam().await;
    }

    secure::delete_steam().await
}

pub async fn forget_expired_steam() -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::delete_steam().await;
    }

    secure::forget_expired_steam().await
}

pub async fn save_humble(session: String) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::save_humble(session).await;
    }

    secure::save_humble(session).await
}

pub async fn delete_humble() -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::delete_humble().await;
    }

    secure::delete_humble().await
}

pub async fn forget_expired_humble() -> Result<(), String> {
    #[cfg(debug_assertions)]
    if development_cache_enabled() {
        return development::delete_humble().await;
    }

    secure::forget_expired_humble().await
}

fn development_cache_value_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(debug_assertions)]
async fn migrate_development_cache(
    mut secure_credentials: DevelopmentCredentials,
) -> Result<DevelopmentCredentials, String> {
    let development_credentials = development::load().await?;
    if development_credentials.steam.is_none() && development_credentials.humble_session.is_none() {
        return Ok(secure_credentials);
    }

    if secure_credentials.steam.is_none() {
        secure_credentials.steam = development_credentials.steam;
    }
    if secure_credentials.humble_session.is_none() {
        secure_credentials.humble_session = development_credentials.humble_session;
    }
    secure::save_all(secure_credentials.clone()).await?;
    development::delete_all().await?;

    #[cfg(debug_assertions)]
    eprintln!("DEVELOPMENT_CREDENTIAL_CACHE=migrated-to-system-store");
    Ok(secure_credentials)
}

mod secure {
    #[cfg(target_os = "macos")]
    use super::CREDENTIALS_ENTRY;
    use super::{DevelopmentCredentials, KEYRING_SERVICE, SavedSteamLogin};
    #[cfg(not(target_os = "macos"))]
    use super::{HUMBLE_SESSION_ENTRY, STEAM_ACCESS_TOKEN_ENTRY, STEAM_LOGIN_ENTRY};
    use keyring::{Entry, Error};
    #[cfg(target_os = "macos")]
    use std::future::Future;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    #[cfg(not(target_os = "macos"))]
    #[derive(Deserialize, Serialize)]
    struct StoredSteamLogin {
        account_name: String,
        refresh_token: String,
    }

    #[cfg(not(target_os = "macos"))]
    use serde::{Deserialize, Serialize};

    pub async fn load() -> Result<DevelopmentCredentials, String> {
        #[cfg(target_os = "macos")]
        {
            let mut snapshot = credential_snapshot().lock().await;
            load_snapshot_once(&mut snapshot, run_blocking(load_macos_blocking)).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(load_legacy_credentials).await
        }
    }

    #[cfg(debug_assertions)]
    pub async fn save_all(credentials: DevelopmentCredentials) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            replace_macos_credentials(credentials).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(move || save_all_legacy_blocking(credentials)).await
        }
    }

    pub async fn save_steam(login: SavedSteamLogin) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_credentials(move |credentials| {
                credentials.steam = Some(login);
            })
            .await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(move || save_steam_legacy_blocking(login)).await
        }
    }

    pub async fn delete_steam() -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_credentials(|credentials| credentials.steam = None).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(delete_steam_legacy_blocking).await
        }
    }

    pub async fn forget_expired_steam() -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_snapshot(|credentials| credentials.steam = None).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            delete_steam().await
        }
    }

    pub async fn save_humble(session: String) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_credentials(move |credentials| {
                credentials.humble_session = Some(session);
            })
            .await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(move || set_password(HUMBLE_SESSION_ENTRY, &session)).await
        }
    }

    pub async fn delete_humble() -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_credentials(|credentials| credentials.humble_session = None).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _guard = mutation_gate().lock().await;
            run_blocking(|| delete_entry(HUMBLE_SESSION_ENTRY)).await
        }
    }

    pub async fn forget_expired_humble() -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            update_macos_snapshot(|credentials| credentials.humble_session = None).await
        }

        #[cfg(not(target_os = "macos"))]
        {
            delete_humble().await
        }
    }

    #[cfg(target_os = "macos")]
    fn credential_snapshot() -> &'static Mutex<Option<DevelopmentCredentials>> {
        static SNAPSHOT: OnceLock<Mutex<Option<DevelopmentCredentials>>> = OnceLock::new();
        SNAPSHOT.get_or_init(|| Mutex::new(None))
    }

    #[cfg(target_os = "macos")]
    async fn load_snapshot_once<F>(
        snapshot: &mut Option<DevelopmentCredentials>,
        load: F,
    ) -> Result<DevelopmentCredentials, String>
    where
        F: Future<Output = Result<DevelopmentCredentials, String>>,
    {
        if let Some(credentials) = snapshot.as_ref() {
            return Ok(credentials.clone());
        }
        let credentials = load.await?;
        *snapshot = Some(credentials.clone());
        Ok(credentials)
    }

    #[cfg(target_os = "macos")]
    async fn update_macos_credentials(
        update: impl FnOnce(&mut DevelopmentCredentials),
    ) -> Result<(), String> {
        let mut snapshot = credential_snapshot().lock().await;
        let current = load_snapshot_once(&mut snapshot, run_blocking(load_macos_blocking)).await?;
        let mut updated = current.clone();
        update(&mut updated);
        if updated == current {
            return Ok(());
        }
        let stored = updated.clone();
        run_blocking(move || save_macos_blocking(&stored)).await?;
        *snapshot = Some(updated);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn update_macos_snapshot(
        update: impl FnOnce(&mut DevelopmentCredentials),
    ) -> Result<(), String> {
        let mut snapshot = credential_snapshot().lock().await;
        let mut credentials =
            load_snapshot_once(&mut snapshot, run_blocking(load_macos_blocking)).await?;
        update(&mut credentials);
        // Automatic expiry handling must not trigger another Keychain prompt.
        // Keep the stale value only in Keychain until an explicit disconnect or
        // a replacement login writes the consolidated entry again.
        *snapshot = Some(credentials);
        Ok(())
    }

    #[cfg(all(debug_assertions, target_os = "macos"))]
    async fn replace_macos_credentials(credentials: DevelopmentCredentials) -> Result<(), String> {
        let mut snapshot = credential_snapshot().lock().await;
        let current = load_snapshot_once(&mut snapshot, run_blocking(load_macos_blocking)).await?;
        if credentials == current {
            return Ok(());
        }
        let stored = credentials.clone();
        run_blocking(move || save_macos_blocking(&stored)).await?;
        *snapshot = Some(credentials);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn load_macos_blocking() -> Result<DevelopmentCredentials, String> {
        read_optional(CREDENTIALS_ENTRY)?
            .map(|encoded| decode_credentials(&encoded))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    #[cfg(target_os = "macos")]
    fn save_macos_blocking(credentials: &DevelopmentCredentials) -> Result<(), String> {
        if credentials.steam.is_none() && credentials.humble_session.is_none() {
            return delete_entry(CREDENTIALS_ENTRY);
        }
        set_password(CREDENTIALS_ENTRY, &encode_credentials(credentials)?)
    }

    #[cfg(not(target_os = "macos"))]
    fn load_legacy_credentials() -> Result<DevelopmentCredentials, String> {
        let stored_login = read_optional(STEAM_LOGIN_ENTRY)?
            .map(|encoded| {
                serde_json::from_str::<StoredSteamLogin>(&encoded)
                    .map_err(|_| "The saved Steam login is unreadable.".to_string())
            })
            .transpose()?;
        let access_token = read_optional(STEAM_ACCESS_TOKEN_ENTRY)?;
        let steam = stored_login.map(|login| SavedSteamLogin {
            account_name: login.account_name,
            refresh_token: login.refresh_token,
            access_token,
        });
        let humble_session = read_optional(HUMBLE_SESSION_ENTRY)?;
        Ok(DevelopmentCredentials {
            steam,
            humble_session,
        })
    }

    #[cfg(all(debug_assertions, not(target_os = "macos")))]
    fn save_all_legacy_blocking(credentials: DevelopmentCredentials) -> Result<(), String> {
        match credentials.steam {
            Some(login) => save_steam_legacy_blocking(login)?,
            None => delete_steam_legacy_blocking()?,
        }
        match credentials.humble_session {
            Some(session) => set_password(HUMBLE_SESSION_ENTRY, &session),
            None => delete_entry(HUMBLE_SESSION_ENTRY),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn save_steam_legacy_blocking(login: SavedSteamLogin) -> Result<(), String> {
        let stored = StoredSteamLogin {
            account_name: login.account_name,
            refresh_token: login.refresh_token,
        };
        let encoded = serde_json::to_string(&stored)
            .map_err(|_| "Could not encode the Steam login.".to_string())?;
        set_password(STEAM_LOGIN_ENTRY, &encoded)?;
        match login.access_token {
            Some(access_token) => set_password(STEAM_ACCESS_TOKEN_ENTRY, &access_token),
            None => delete_entry(STEAM_ACCESS_TOKEN_ENTRY),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn delete_steam_legacy_blocking() -> Result<(), String> {
        delete_entry(STEAM_LOGIN_ENTRY)?;
        delete_entry(STEAM_ACCESS_TOKEN_ENTRY)
    }

    #[cfg(target_os = "macos")]
    fn encode_credentials(credentials: &DevelopmentCredentials) -> Result<String, String> {
        serde_json::to_string(credentials)
            .map_err(|_| "Could not encode the saved credentials.".to_string())
    }

    #[cfg(target_os = "macos")]
    fn decode_credentials(encoded: &str) -> Result<DevelopmentCredentials, String> {
        serde_json::from_str(encoded)
            .map_err(|_| "The saved credentials are unreadable.".to_string())
    }

    #[cfg(not(target_os = "macos"))]
    fn mutation_gate() -> &'static Mutex<()> {
        static GATE: OnceLock<Mutex<()>> = OnceLock::new();
        GATE.get_or_init(|| Mutex::new(()))
    }

    async fn run_blocking<T>(
        task: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String>
    where
        T: Send + 'static,
    {
        tauri::async_runtime::spawn_blocking(task)
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    fn entry(name: &str) -> Result<Entry, String> {
        Entry::new(KEYRING_SERVICE, name)
            .map_err(|error| format!("Could not access the system credential store: {error}"))
    }

    fn read_optional(name: &str) -> Result<Option<String>, String> {
        match entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(format!(
                "Could not read from the system credential store: {error}"
            )),
        }
    }

    fn set_password(name: &str, value: &str) -> Result<(), String> {
        entry(name)?
            .set_password(value)
            .map_err(|error| format!("Could not save to the system credential store: {error}"))
    }

    fn delete_entry(name: &str) -> Result<(), String> {
        match entry(name)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(format!(
                "Could not remove a system credential-store entry: {error}"
            )),
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    mod tests {
        use super::*;

        #[test]
        fn consolidated_credentials_round_trip() {
            let credentials = DevelopmentCredentials {
                steam: Some(SavedSteamLogin {
                    account_name: "tester".to_string(),
                    refresh_token: "refresh".to_string(),
                    access_token: Some("access".to_string()),
                }),
                humble_session: Some("humble".to_string()),
            };

            let decoded = decode_credentials(&encode_credentials(&credentials).unwrap()).unwrap();
            let steam = decoded.steam.unwrap();
            assert_eq!(steam.account_name, "tester");
            assert_eq!(steam.refresh_token, "refresh");
            assert_eq!(steam.access_token.as_deref(), Some("access"));
            assert_eq!(decoded.humble_session.as_deref(), Some("humble"));
        }

        #[tokio::test]
        async fn credential_snapshot_is_loaded_only_once() {
            use std::cell::Cell;

            let loads = Cell::new(0);
            let mut snapshot = None;
            let first = load_snapshot_once(&mut snapshot, async {
                loads.set(loads.get() + 1);
                Ok(DevelopmentCredentials::default())
            })
            .await
            .unwrap();
            let second = load_snapshot_once(&mut snapshot, async {
                loads.set(loads.get() + 1);
                Ok(DevelopmentCredentials::default())
            })
            .await
            .unwrap();

            assert_eq!(loads.get(), 1);
            assert_eq!(first, second);
        }
    }
}

#[cfg(debug_assertions)]
mod development {
    use super::{DevelopmentCredentials, SavedSteamLogin};
    use std::io::{ErrorKind, Write};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    pub async fn load() -> Result<DevelopmentCredentials, String> {
        load_unlocked().await
    }

    async fn load_unlocked() -> Result<DevelopmentCredentials, String> {
        tauri::async_runtime::spawn_blocking(|| {
            let path = credentials_path()?;
            match std::fs::read_to_string(path) {
                Ok(value) => serde_json::from_str(&value)
                    .map_err(|_| "The development credential cache is unreadable".to_string()),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    Ok(DevelopmentCredentials::default())
                }
                Err(error) => Err(format!(
                    "Could not read the development credential cache: {error}"
                )),
            }
        })
        .await
        .map_err(|error| format!("Credential-cache task failed: {error}"))?
    }

    pub async fn save_steam(login: SavedSteamLogin) -> Result<(), String> {
        let _guard = mutation_gate().lock().await;
        let mut credentials = load_unlocked().await?;
        credentials.steam = Some(login);
        save_unlocked(credentials).await
    }

    pub async fn delete_steam() -> Result<(), String> {
        let _guard = mutation_gate().lock().await;
        let mut credentials = load_unlocked().await?;
        credentials.steam = None;
        save_or_delete_unlocked(credentials).await
    }

    pub async fn save_humble(session: String) -> Result<(), String> {
        let _guard = mutation_gate().lock().await;
        let mut credentials = load_unlocked().await?;
        credentials.humble_session = Some(session);
        save_unlocked(credentials).await
    }

    pub async fn delete_humble() -> Result<(), String> {
        let _guard = mutation_gate().lock().await;
        let mut credentials = load_unlocked().await?;
        credentials.humble_session = None;
        save_or_delete_unlocked(credentials).await
    }

    pub async fn delete_all() -> Result<(), String> {
        let _guard = mutation_gate().lock().await;
        delete_all_unlocked().await
    }

    async fn delete_all_unlocked() -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(|| match std::fs::remove_file(credentials_path()?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Could not remove the development credential cache: {error}"
            )),
        })
        .await
        .map_err(|error| format!("Credential-cache task failed: {error}"))?
    }

    async fn save_or_delete_unlocked(credentials: DevelopmentCredentials) -> Result<(), String> {
        if credentials.steam.is_none() && credentials.humble_session.is_none() {
            return delete_all_unlocked().await;
        }
        save_unlocked(credentials).await
    }

    async fn save_unlocked(credentials: DevelopmentCredentials) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || {
            let path = credentials_path()?;
            let parent = path
                .parent()
                .ok_or_else(|| "Development credential path has no parent".to_string())?;
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create the credential directory: {error}"))?;
            protect_directory(parent)?;

            let encoded = serde_json::to_vec(&credentials)
                .map_err(|_| "Could not encode the development credentials".to_string())?;
            let mut file = open_credentials_file(&path)?;
            file.write_all(&encoded)
                .map_err(|error| format!("Could not save development credentials: {error}"))
        })
        .await
        .map_err(|error| format!("Credential-cache task failed: {error}"))?
    }

    fn mutation_gate() -> &'static Mutex<()> {
        static GATE: OnceLock<Mutex<()>> = OnceLock::new();
        GATE.get_or_init(|| Mutex::new(()))
    }

    #[cfg(unix)]
    fn protect_directory(path: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not protect the credential directory: {error}"))
    }

    #[cfg(windows)]
    fn protect_directory(_path: &Path) -> Result<(), String> {
        Ok(())
    }

    fn open_credentials_file(path: &Path) -> Result<std::fs::File, String> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let file = options
            .open(path)
            .map_err(|error| format!("Could not open the credential cache: {error}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("Could not protect the credential cache: {error}"))?;
        }

        Ok(file)
    }

    fn credentials_path() -> Result<PathBuf, String> {
        dirs_next::data_local_dir()
            .map(|directory| {
                directory
                    .join("Humble Gift Matcher")
                    .join("dev-credentials.json")
            })
            .ok_or_else(|| "Could not locate the local application-data directory".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_cache_requires_an_explicit_truthy_value() {
        for value in ["1", "true", "TRUE", "yes", "on", " On "] {
            assert!(development_cache_value_enabled(value));
        }
        for value in ["", "0", "false", "off", "anything"] {
            assert!(!development_cache_value_enabled(value));
        }
    }
}
