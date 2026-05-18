use std::path::{Path, PathBuf};

use super::{ProjectManifest, WallpaperProjectType, validate_relative_normal_path};

const PROJECT_FILE_NAME: &str = "project.json";
const DEFAULT_SCENE_ENTRY: &str = "scene.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneSourcePaths {
    pub pkg_path: PathBuf,
    pub pkg_entry: String,
    pub pkg_dir: PathBuf,
    pub scene_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneSourceResolution {
    Scene {
        manifest: ProjectManifest,
        scene_source: SceneSourcePaths,
    },
    NotSceneProject {
        manifest: ProjectManifest,
    },
}

impl SceneSourceResolution {
    /// Resolves a project manifest or scene file into renderer source paths.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the source path is
    /// empty, the manifest cannot be loaded, or the scene entry is not a
    /// safe relative path.
    pub fn load<T: AsRef<Path>>(project_path: T) -> Result<Self, crate::EngineError> {
        let source_path = project_path.as_ref();
        if source_path.as_os_str().is_empty() {
            return Err(crate::EngineError::InvalidInput(
                "scene source must not be empty".to_string(),
            ));
        }

        if source_path.file_name().and_then(|name| name.to_str()) == Some(PROJECT_FILE_NAME) {
            let manifest = ProjectManifest::load(source_path)?;
            if manifest.project_type() != WallpaperProjectType::Scene {
                return Ok(Self::NotSceneProject { manifest });
            }

            let entry = if manifest.file().is_empty() {
                PathBuf::from(DEFAULT_SCENE_ENTRY)
            } else {
                PathBuf::from(manifest.file())
            };
            let scene_source = Self::resolve_scene_entry(source_path, &entry)?;
            return Ok(Self::Scene {
                manifest,
                scene_source,
            });
        }

        let mut manifest = ProjectManifest::default();
        manifest.set_project_type(WallpaperProjectType::Scene);
        manifest.set_file(
            source_path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        manifest.set_workshop_id(
            source_path
                .parent()
                .and_then(Path::file_name)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );

        let entry = source_path.file_name().map(PathBuf::from).ok_or_else(|| {
            crate::EngineError::InvalidInput("scene source must include a file name".to_string())
        })?;
        let scene_source = Self::resolve_scene_entry(source_path, &entry)?;
        Ok(Self::Scene {
            manifest,
            scene_source,
        })
    }

    #[must_use]
    pub fn scene_source(&self) -> Option<&SceneSourcePaths> {
        match self {
            Self::Scene { scene_source, .. } => Some(scene_source),
            Self::NotSceneProject { .. } => None,
        }
    }

    fn resolve_scene_entry(
        source_path: &Path,
        entry_path: &Path,
    ) -> Result<SceneSourcePaths, crate::EngineError> {
        validate_relative_normal_path(entry_path, "project scene entry")?;

        let pkg_dir = source_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        let mut pkg_path = pkg_dir.join(entry_path);
        pkg_path.set_extension("pkg");

        let mut pkg_entry = entry_path.to_path_buf();
        pkg_entry.set_extension("json");

        Ok(SceneSourcePaths {
            pkg_path,
            pkg_entry: pkg_entry.to_string_lossy().replace('\\', "/"),
            scene_id: pkg_dir
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
            pkg_dir,
        })
    }
}

impl ProjectManifest {
    pub fn set_project_type(&mut self, project_type: WallpaperProjectType) {
        self.project_type = project_type;
    }

    pub fn set_file(&mut self, file: String) {
        self.file = file;
    }

    pub fn set_workshop_id(&mut self, workshop_id: String) {
        self.workshop_id = workshop_id;
    }
}
