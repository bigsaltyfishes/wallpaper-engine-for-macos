use std::path::PathBuf;

pub const BUNDLE_IDENTIFIER: &str = "dev.molyuu.wallpaper-engine";

#[derive(Clone, Debug)]
pub struct BridgePaths {
    home: Option<PathBuf>,
}

impl Default for BridgePaths {
    fn default() -> Self {
        Self {
            home: dirs::home_dir(),
        }
    }
}

impl BridgePaths {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn for_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
        }
    }

    #[must_use]
    pub fn app_support_root(&self) -> PathBuf {
        self.home.as_deref().map_or_else(
            || PathBuf::from(".").join(BUNDLE_IDENTIFIER),
            |home| {
                home.join("Library")
                    .join("Application Support")
                    .join(BUNDLE_IDENTIFIER)
            },
        )
    }

    #[must_use]
    pub fn steam_workshop_root(&self) -> PathBuf {
        self.home.as_deref().map_or_else(
            || PathBuf::from("/missing/workshop"),
            |home| home.join("Library/Application Support/Steam/steamapps/workshop/content/431960"),
        )
    }

    #[must_use]
    pub fn assets_root(&self) -> PathBuf {
        self.home.as_deref().map_or_else(
            || PathBuf::from("/missing/assets"),
            |home| {
                home.join(
                    "Library/Application Support/Steam/steamapps/common/wallpaper_engine/assets",
                )
            },
        )
    }

    #[must_use]
    pub fn shader_cache_root(&self) -> PathBuf {
        self.app_support_root().join("shader-cache")
    }
}
