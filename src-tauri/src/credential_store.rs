use serde::{Deserialize, Serialize};

const DEVELOPMENT_CACHE_ENV: &str = "HGM_INSECURE_DEV_CREDENTIAL_CACHE";
const KEYRING_SERVICE: &str = "com.jakemartin.humble-gift-matcher";
const STEAM_LOGIN_ENTRY: &str = "steam-login";
const STEAM_ACCESS_TOKEN_ENTRY: &str = "steam-access-token";
const HUMBLE_SESSION_ENTRY: &str = "humble-session";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SavedSteamLogin {
    pub account_name: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
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
    use super::{
        DevelopmentCredentials, HUMBLE_SESSION_ENTRY, KEYRING_SERVICE, STEAM_ACCESS_TOKEN_ENTRY,
        STEAM_LOGIN_ENTRY, SavedSteamLogin,
    };
    use keyring::{Entry, Error};

    #[derive(Deserialize, Serialize)]
    struct StoredSteamLogin {
        account_name: String,
        refresh_token: String,
    }

    use serde::{Deserialize, Serialize};

    pub async fn load() -> Result<DevelopmentCredentials, String> {
        tauri::async_runtime::spawn_blocking(load_blocking)
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    #[cfg(debug_assertions)]
    pub async fn save_all(credentials: DevelopmentCredentials) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || {
            match credentials.steam {
                Some(login) => save_steam_blocking(login)?,
                None => delete_steam_blocking()?,
            }
            match credentials.humble_session {
                Some(session) => set_password(HUMBLE_SESSION_ENTRY, &session)?,
                None => delete_entry(HUMBLE_SESSION_ENTRY)?,
            }
            Ok(())
        })
        .await
        .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    pub async fn save_steam(login: SavedSteamLogin) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || save_steam_blocking(login))
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    pub async fn delete_steam() -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(delete_steam_blocking)
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    pub async fn save_humble(session: String) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || set_password(HUMBLE_SESSION_ENTRY, &session))
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    pub async fn delete_humble() -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(|| delete_entry(HUMBLE_SESSION_ENTRY))
            .await
            .map_err(|error| format!("System credential-store task failed: {error}"))?
    }

    fn load_blocking() -> Result<DevelopmentCredentials, String> {
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

    fn save_steam_blocking(login: SavedSteamLogin) -> Result<(), String> {
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

    fn delete_steam_blocking() -> Result<(), String> {
        delete_entry(STEAM_LOGIN_ENTRY)?;
        delete_entry(STEAM_ACCESS_TOKEN_ENTRY)
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
