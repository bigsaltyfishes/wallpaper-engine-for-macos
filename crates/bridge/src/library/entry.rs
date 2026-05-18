use std::{path::PathBuf, time::SystemTime};

use wallpaper_core::project::WallpaperProjectType;

use crate::project::ProjectModel;

#[derive(Clone, Debug, PartialEq)]
pub struct WallpaperEntry {
    pub workshop_id: String,
    pub title: String,
    pub project_type: WallpaperProjectType,
    pub preview_path: Option<PathBuf>,
    pub preview_mtime: Option<SystemTime>,
    pub project_json_mtime: Option<SystemTime>,
    pub supported: bool,
}

impl WallpaperEntry {
    #[must_use]
    pub fn from_model(model: &ProjectModel) -> Self {
        let preview_mtime = model
            .preview_file
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok());

        Self {
            workshop_id: model.workshop_id.clone(),
            title: model.title.clone(),
            project_type: model.project_type,
            preview_path: model.preview_file.clone(),
            preview_mtime,
            project_json_mtime: None,
            supported: matches!(
                model.project_type,
                WallpaperProjectType::Scene | WallpaperProjectType::Video
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    use super::*;

    fn model(project_type: WallpaperProjectType) -> ProjectModel {
        ProjectModel {
            workshop_id: "123".to_string(),
            title: "Test Wallpaper".to_string(),
            description_html: String::new(),
            project_type,
            preview_file: None,
            properties: Vec::new(),
        }
    }

    #[test]
    fn scene_is_supported() {
        let entry = WallpaperEntry::from_model(&model(WallpaperProjectType::Scene));
        assert!(entry.supported);
    }

    #[test]
    fn web_is_unsupported() {
        let entry = WallpaperEntry::from_model(&model(WallpaperProjectType::Web));
        assert!(!entry.supported);
    }

    #[test]
    fn video_is_supported() {
        let entry = WallpaperEntry::from_model(&model(WallpaperProjectType::Video));
        assert!(entry.supported);
    }

    #[test]
    fn unknown_is_unsupported() {
        let entry = WallpaperEntry::from_model(&model(WallpaperProjectType::Unknown));
        assert!(!entry.supported);
    }

    #[test]
    fn preview_mtime_is_probed_when_preview_exists() {
        let temp = TempDir::new().unwrap();
        let preview = temp.path().join("preview.gif");
        fs::write(&preview, b"preview").unwrap();

        let mut model = model(WallpaperProjectType::Scene);
        model.preview_file = Some(preview);

        let entry = WallpaperEntry::from_model(&model);
        assert!(entry.preview_mtime.is_some());
    }

    #[test]
    fn preview_mtime_is_none_when_preview_missing() {
        let mut model = model(WallpaperProjectType::Scene);
        model.preview_file = Some(PathBuf::from("missing.gif"));

        let entry = WallpaperEntry::from_model(&model);
        assert_eq!(entry.preview_mtime, None);
    }
}
