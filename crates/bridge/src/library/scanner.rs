use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{BridgeError, BridgeErrorKind, library::WallpaperEntry, project::ProjectModel};

/// # Errors
///
/// Returns an error when the workshop root is missing or unreadable.
pub fn scan(root: &Path) -> Result<Vec<WallpaperEntry>, BridgeError> {
    if !root.exists() {
        return Err(BridgeError::Error {
            kind: BridgeErrorKind::Library,
            message: format!("workshop root missing: {}", root.display()),
        });
    }

    let children = fs::read_dir(root).map_err(|error| BridgeError::Error {
        kind: BridgeErrorKind::Io,
        message: format!("workshop root unreadable: {}: {error}", root.display()),
    })?;
    let mut entries = Vec::new();

    for child in children {
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                log::warn!(
                    "skipped unreadable workshop child root={} error={error}",
                    root.display()
                );
                continue;
            }
        };

        let file_type = match child.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                log::warn!(
                    "skipped workshop child with unreadable file type path={} error={error}",
                    child.path().display()
                );
                continue;
            }
        };

        if !file_type.is_dir() {
            log::debug!(
                "skipped non-directory workshop child path={}",
                child.path().display()
            );
            continue;
        }

        let folder = child.path();
        let project_json = folder.join("project.json");
        if !project_json.is_file() {
            log::debug!(
                "skipped workshop folder without project.json path={}",
                folder.display()
            );
            continue;
        }

        let workshop_id = child.file_name().to_string_lossy().into_owned();
        let model = match ProjectModel::load(&workshop_id, &project_json) {
            Ok(model) => model,
            Err(error) => {
                log::warn!(
                    "skipped workshop folder with unloadable project.json workshop_id={} path={} \
                     error={error}",
                    workshop_id,
                    project_json.display()
                );
                continue;
            }
        };

        let mut entry = WallpaperEntry::from_model(&model);
        entry.project_json_mtime = fs::metadata(&project_json)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        entries.push(entry);
    }

    entries.sort_by(|left, right| {
        left.workshop_id
            .cmp(&right.workshop_id)
            .then_with(|| left.title.cmp(&right.title))
    });
    Ok(entries)
}

#[must_use]
pub fn resolve_preview(workshop_folder: &Path) -> Option<PathBuf> {
    ["preview.gif", "preview.jpg", "preview.jpeg", "preview.png"]
        .into_iter()
        .map(|file_name| workshop_folder.join(file_name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use wallpaper_core::project::WallpaperProjectType;

    use super::*;

    const VALID_MANIFEST: &str = r#"{
        "type": "scene",
        "title": "Valid Wallpaper",
        "preview": "preview.png"
    }"#;

    fn write_manifest(dir: &Path, contents: &str) {
        std::fs::write(dir.join("project.json"), contents).unwrap();
    }

    #[test]
    fn scan_skips_folders_without_manifest() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("123")).unwrap();

        let entries = scan(temp.path()).unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn scan_returns_workshop_missing_for_missing_root() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing");

        let err = scan(&missing).unwrap_err();

        assert_eq!(err.kind(), BridgeErrorKind::Library);
        assert!(err.message().contains(&missing.display().to_string()));
    }

    #[test]
    fn scan_skips_malformed_manifests() {
        let temp = TempDir::new().unwrap();
        let malformed = temp.path().join("123");
        let valid = temp.path().join("456");
        std::fs::create_dir(&malformed).unwrap();
        std::fs::create_dir(&valid).unwrap();
        write_manifest(&malformed, "not-json");
        write_manifest(&valid, VALID_MANIFEST);

        let entries = scan(temp.path()).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].workshop_id, "456");
        assert_eq!(entries[0].title, "Valid Wallpaper");
        assert!(entries[0].project_json_mtime.is_some());
    }

    #[test]
    fn scan_skips_non_directory_children() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("not-a-workshop"), b"file").unwrap();
        let valid = temp.path().join("456");
        std::fs::create_dir(&valid).unwrap();
        write_manifest(&valid, VALID_MANIFEST);

        let entries = scan(temp.path()).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].workshop_id, "456");
    }

    #[test]
    fn scan_populates_entry_from_valid_manifest() {
        let temp = TempDir::new().unwrap();
        let folder = temp.path().join("456");
        std::fs::create_dir(&folder).unwrap();
        write_manifest(&folder, VALID_MANIFEST);
        std::fs::write(folder.join("preview.png"), b"preview").unwrap();

        let entries = scan(temp.path()).unwrap();

        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.project_type, WallpaperProjectType::Scene);
        assert!(entry.supported);
        assert_eq!(entry.preview_path, Some(folder.join("preview.png")));
        assert!(entry.preview_mtime.is_some());
    }

    #[test]
    fn resolve_preview_prefers_gif() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("preview.png"), b"png").unwrap();
        std::fs::write(temp.path().join("preview.gif"), b"gif").unwrap();

        let preview = resolve_preview(temp.path());

        assert_eq!(preview, Some(temp.path().join("preview.gif")));
    }

    #[test]
    fn resolve_preview_returns_none_when_absent() {
        let temp = TempDir::new().unwrap();

        let preview = resolve_preview(temp.path());

        assert_eq!(preview, None);
    }

    #[test]
    fn resolve_preview_ignores_directories() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("preview.gif")).unwrap();

        let preview = resolve_preview(temp.path());

        assert_eq!(preview, None);
    }
}
