use wallpaper_core::project::WallpaperProjectType;

use crate::{config::FilterCfg, library::WallpaperEntry};

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeFilter {
    pub scene: bool,
    pub video: bool,
    pub web: bool,
    pub unknown: bool,
}

impl Default for TypeFilter {
    fn default() -> Self {
        Self::from_config(&FilterCfg::default())
    }
}

impl TypeFilter {
    #[must_use]
    pub fn from_config(c: &FilterCfg) -> Self {
        Self {
            scene: c.scene,
            video: c.video,
            web: c.web,
            unknown: c.unknown,
        }
    }

    #[must_use]
    pub fn to_config(&self) -> FilterCfg {
        FilterCfg {
            scene: self.scene,
            video: self.video,
            web: self.web,
            unknown: self.unknown,
        }
    }

    #[must_use]
    pub fn accepts(&self, entry: &WallpaperEntry) -> bool {
        match entry.project_type {
            WallpaperProjectType::Scene => self.scene,
            WallpaperProjectType::Video => self.video,
            WallpaperProjectType::Web => self.web,
            WallpaperProjectType::Unknown => self.unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wallpaper_core::project::WallpaperProjectType;

    use crate::{
        config::FilterCfg,
        library::{TypeFilter, WallpaperEntry},
    };

    fn entry(project_type: WallpaperProjectType) -> WallpaperEntry {
        WallpaperEntry {
            workshop_id: String::new(),
            title: String::new(),
            project_type,
            preview_path: Some(PathBuf::new()),
            preview_mtime: None,
            project_json_mtime: None,
            supported: true,
        }
    }

    #[test]
    fn default_accepts_all_types() {
        let filter = TypeFilter::default();

        assert!(filter.accepts(&entry(WallpaperProjectType::Scene)));
        assert!(filter.accepts(&entry(WallpaperProjectType::Video)));
        assert!(filter.accepts(&entry(WallpaperProjectType::Web)));
        assert!(filter.accepts(&entry(WallpaperProjectType::Unknown)));
    }

    #[test]
    fn scene_only_filter_accepts_only_scene() {
        let filter = TypeFilter {
            scene: true,
            video: false,
            web: false,
            unknown: false,
        };

        assert!(filter.accepts(&entry(WallpaperProjectType::Scene)));
        assert!(!filter.accepts(&entry(WallpaperProjectType::Video)));
        assert!(!filter.accepts(&entry(WallpaperProjectType::Web)));
        assert!(!filter.accepts(&entry(WallpaperProjectType::Unknown)));
    }

    #[test]
    fn round_trips_config_shape() {
        let config = FilterCfg {
            scene: false,
            video: true,
            web: false,
            unknown: true,
        };

        let filter = TypeFilter::from_config(&config);

        assert_eq!(filter.to_config(), config);
    }
}
