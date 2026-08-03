use serde::{Deserialize, Serialize};

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

    async fn save_or_delete_unlocked(credentials: DevelopmentCredentials) -> Result<(), String> {
        if credentials.steam.is_none() && credentials.humble_session.is_none() {
            return tauri::async_runtime::spawn_blocking(|| {
                match std::fs::remove_file(credentials_path()?) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(format!(
                        "Could not remove the development credential cache: {error}"
                    )),
                }
            })
            .await
            .map_err(|error| format!("Credential-cache task failed: {error}"))?;
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

#[cfg(debug_assertions)]
pub use development::{delete_humble, delete_steam, load, save_humble, save_steam};

#[cfg(not(debug_assertions))]
pub async fn load() -> Result<DevelopmentCredentials, String> {
    Ok(DevelopmentCredentials::default())
}

#[cfg(not(debug_assertions))]
pub async fn save_steam(_login: SavedSteamLogin) -> Result<(), String> {
    Err(
        "Credential persistence is unavailable until the OS credential store is enabled"
            .to_string(),
    )
}

#[cfg(not(debug_assertions))]
pub async fn delete_steam() -> Result<(), String> {
    Ok(())
}

#[cfg(not(debug_assertions))]
pub async fn save_humble(_session: String) -> Result<(), String> {
    Err(
        "Credential persistence is unavailable until the OS credential store is enabled"
            .to_string(),
    )
}

#[cfg(not(debug_assertions))]
pub async fn delete_humble() -> Result<(), String> {
    Ok(())
}
