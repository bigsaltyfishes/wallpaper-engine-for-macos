use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    app::{self, AppConfig},
    wallpaper::{self, WallpaperConfig},
};
use crate::{BridgeError, BridgeErrorKind, paths::BridgePaths};

#[derive(Debug)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub backup_reported: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    root: PathBuf,
}

impl ConfigStore {
    #[must_use]
    pub fn default_root() -> PathBuf {
        BridgePaths::new().app_support_root()
    }

    #[must_use]
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// # Errors
    ///
    /// Returns an error when the config file is from a newer schema or a
    /// corrupt file cannot be backed up.
    pub fn load(&self) -> Result<ConfigLoad, BridgeError> {
        let path = self.config_path();
        if !path.exists() {
            let config = AppConfig::default();
            return Ok(ConfigLoad {
                config,
                backup_reported: Vec::new(),
            });
        }

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                let backup = backup_corrupted(&path)?;
                let config = AppConfig::default();
                log::warn!(
                    "backed up unreadable app config path={} backup={} error={error}",
                    path.display(),
                    backup.display()
                );
                return Ok(ConfigLoad {
                    config,
                    backup_reported: vec![backup],
                });
            }
        };

        match toml::from_str::<AppConfig>(&raw) {
            Ok(config) => {
                if config.schema_version > app::SCHEMA_VERSION {
                    return Err(config_error(format!(
                        "config schema version {} is newer than supported version {}",
                        config.schema_version,
                        app::SCHEMA_VERSION
                    )));
                }

                Ok(ConfigLoad {
                    config,
                    backup_reported: Vec::new(),
                })
            }
            Err(error) => {
                let backup = backup_corrupted(&path)?;
                let config = AppConfig::default();
                log::warn!(
                    "backed up corrupted app config path={} backup={} error={error}",
                    path.display(),
                    backup.display()
                );
                Ok(ConfigLoad {
                    config,
                    backup_reported: vec![backup],
                })
            }
        }
    }

    /// # Errors
    ///
    /// Returns an error when the config cannot be serialized or written
    /// atomically.
    pub fn save_app_config(&self, config: &AppConfig) -> Result<(), BridgeError> {
        let bytes = toml::to_string_pretty(config)
            .map_err(|error| config_error(error.to_string()))?
            .into_bytes();
        super::writer::atomic_write(&self.config_path(), &bytes)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper config cannot be read, parsed, or
    /// backed up.
    pub fn load_wallpaper(&self, id: &str) -> Result<WallpaperConfig, BridgeError> {
        let path = self.wallpaper_path(id);
        if !path.exists() {
            return Ok(WallpaperConfig::new_for(id, "scene"));
        }

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                let _backup = backup_corrupted(&path)?;
                return Err(config_error(format!(
                    "failed to read wallpaper config {id}: {error}"
                )));
            }
        };
        let config = match serde_json::from_str::<WallpaperConfig>(&raw) {
            Ok(config) => config,
            Err(error) => {
                let _backup = backup_corrupted(&path)?;
                return Err(config_error(format!(
                    "corrupted wallpaper config {id}: {error}"
                )));
            }
        };

        if config.schema_version > wallpaper::SCHEMA_VERSION {
            return Err(config_error(format!(
                "wallpaper config schema version {} is newer than supported version {}",
                config.schema_version,
                wallpaper::SCHEMA_VERSION
            )));
        }

        Ok(config)
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper config cannot be serialized or
    /// written atomically.
    pub fn save_wallpaper(&self, config: &WallpaperConfig) -> Result<(), BridgeError> {
        let bytes = serde_json::to_string_pretty(config)
            .map_err(|error| config_error(error.to_string()))?
            .into_bytes();
        super::writer::atomic_write(&self.wallpaper_path(&config.workshop_id), &bytes)?;
        Ok(())
    }

    /// # Errors
    ///
    /// This compatibility shim currently does not fail.
    pub fn flush(&self) -> Result<usize, BridgeError> {
        // Temporary compatibility shim: config writes are immediate.
        Ok(0)
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    fn wallpaper_path(&self, id: &str) -> PathBuf {
        self.root.join("wallpapers").join(format!("{id}.json"))
    }
}

fn backup_corrupted(path: &Path) -> Result<PathBuf, BridgeError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .expect("config path has file name")
        .to_string_lossy();
    let backup = path.with_file_name(format!("{file_name}.corrupted-{timestamp}"));
    fs::rename(path, &backup)?;
    Ok(backup)
}

fn config_error(message: impl Into<String>) -> BridgeError {
    BridgeError::Error {
        kind: BridgeErrorKind::Config,
        message: message.into(),
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(error: std::io::Error) -> Self {
        BridgeError::Error {
            kind: BridgeErrorKind::Io,
            message: error.to_string(),
        }
    }
}
