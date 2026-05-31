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
            |home| home.join("/Users/wjj/Pictures/wallpaper"),
        )
    }

    #[must_use]
    pub fn assets_root(&self) -> PathBuf {
        self.home.as_deref().map_or_else(
            || PathBuf::from("/missing/assets"),
            |home| {
                home.join(
                    "/Users/wjj/Pictures/assets_2",
                )
            },
        )
    }

    #[must_use]
    pub fn shader_cache_root(&self) -> PathBuf {
        self.app_support_root().join("shader-cache")
    }

    #[must_use]
    pub fn logs_root(&self) -> PathBuf {
        self.app_support_root().join("Logs")
    }

    #[must_use]
    pub fn log_session_root(&self, start_time: &str) -> PathBuf {
        self.logs_root().join(start_time)
    }
}

#[cfg(test)]
mod tests {
    use super::{BUNDLE_IDENTIFIER, BridgePaths};

    #[test]
    fn logs_root_lives_under_app_support() {
        let paths = BridgePaths::for_home("/Users/example");

        assert_eq!(
            paths.logs_root(),
            std::path::PathBuf::from("/Users/example")
                .join("Library")
                .join("Application Support")
                .join(BUNDLE_IDENTIFIER)
                .join("Logs")
        );
    }

    #[test]
    fn log_session_root_appends_session_name() {
        let paths = BridgePaths::for_home("/Users/example");

        assert_eq!(
            paths.log_session_root("20260520-120000"),
            paths.logs_root().join("20260520-120000")
        );
    }
}
