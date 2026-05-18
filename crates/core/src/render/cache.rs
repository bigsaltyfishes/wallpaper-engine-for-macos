//! Shader cache invalidation policy for scene wallpapers.
//!
//! Open Wallpaper Engine consumes a scene-specific cache directory. Rust owns
//! the policy for choosing that directory and deciding when it must be purged,
//! based on source file modification times and explicit refresh requests.

use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde_json::{Value, json};

/// Inputs needed to prepare one scene's shader cache directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderCacheInputs {
    scene_id: String,
    cache_root: PathBuf,
    project_json_path: PathBuf,
    scene_pkg_path: PathBuf,
    property_override_json: Option<String>,
    force_refresh: bool,
}

impl ShaderCacheInputs {
    /// Starts building cache inputs for `scene_id` under `cache_root`.
    ///
    /// `scene_id` must be a single path component. This prevents a malformed
    /// workshop ID from escaping the cache root.
    #[must_use]
    pub fn builder(
        scene_id: impl Into<String>,
        cache_root: impl Into<PathBuf>,
    ) -> ShaderCacheInputsBuilder {
        ShaderCacheInputsBuilder {
            scene_id: scene_id.into(),
            cache_root: cache_root.into(),
            project_json_path: None,
            scene_pkg_path: None,
            property_override_json: None,
            force_refresh: false,
        }
    }
}

impl AsRef<ShaderCacheInputs> for ShaderCacheInputs {
    fn as_ref(&self) -> &ShaderCacheInputs {
        self
    }
}

/// Builder for [`ShaderCacheInputs`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderCacheInputsBuilder {
    scene_id: String,
    cache_root: PathBuf,
    project_json_path: Option<PathBuf>,
    scene_pkg_path: Option<PathBuf>,
    property_override_json: Option<String>,
    force_refresh: bool,
}

impl ShaderCacheInputsBuilder {
    /// Sets the `project.json` path used for cache manifest freshness.
    #[must_use]
    pub fn project_json_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.project_json_path = Some(path.into());
        self
    }

    /// Sets the `scene.pkg` path used for cache manifest freshness.
    #[must_use]
    pub fn scene_pkg_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.scene_pkg_path = Some(path.into());
        self
    }

    /// Sets the project property override JSON used for cache freshness.
    #[must_use]
    pub fn property_override_json(mut self, json: Option<impl Into<String>>) -> Self {
        self.property_override_json = json.map(Into::into);
        self
    }

    /// Forces the scene cache directory to be recreated.
    #[must_use]
    pub fn force_refresh(mut self, force_refresh: bool) -> Self {
        self.force_refresh = force_refresh;
        self
    }

    /// Validates `self.scene_id`. Kept as a named method (rather than inlined
    /// into `build()`) because the validation is substantive (~20 lines) and
    /// the dedicated name documents its safety purpose: preventing a malformed
    /// workshop id from escaping `cache_root`.
    #[allow(clippy::single_call_fn)]
    fn validate_scene_id(&self) -> Result<(), crate::EngineError> {
        if self.scene_id.is_empty() {
            return Err(crate::EngineError::InvalidInput(
                "scene_id must not be empty".to_string(),
            ));
        }
        if self.scene_id.contains('/') || self.scene_id.contains('\\') {
            return Err(crate::EngineError::InvalidInput(
                "scene_id must be a single path component".to_string(),
            ));
        }

        let mut components = Path::new(&self.scene_id).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(component)), None)
                if component == OsStr::new(&self.scene_id) =>
            {
                Ok(())
            }
            _ => Err(crate::EngineError::InvalidInput(
                "scene_id must be a single path component".to_string(),
            )),
        }
    }

    /// Validates and returns the completed cache inputs.
    ///
    /// # Errors
    ///
    /// Returns an error if the scene id, cache root, project path, or package
    /// path is invalid or missing.
    pub fn build(self) -> Result<ShaderCacheInputs, crate::EngineError> {
        self.validate_scene_id()?;
        if self.cache_root.as_os_str().is_empty() {
            return Err(crate::EngineError::InvalidInput(
                "cache_root must not be empty".to_string(),
            ));
        }

        let project_json_path = self.project_json_path.ok_or_else(|| {
            crate::EngineError::InvalidInput("project_json_path must be set".to_string())
        })?;
        let scene_pkg_path = self.scene_pkg_path.ok_or_else(|| {
            crate::EngineError::InvalidInput("scene_pkg_path must be set".to_string())
        })?;

        Ok(ShaderCacheInputs {
            scene_id: self.scene_id,
            cache_root: self.cache_root,
            project_json_path,
            scene_pkg_path,
            property_override_json: self.property_override_json,
            force_refresh: self.force_refresh,
        })
    }
}

/// Result of preparing a scene shader cache directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderCacheDecision {
    scene_cache_path: PathBuf,
    purged_cache: bool,
}

impl ShaderCacheDecision {
    /// Prepares the cache directory and returns the decision that was applied.
    ///
    /// If the manifest changed or `force_refresh` is set, the existing scene
    /// cache is removed before the manifest is rewritten.
    ///
    /// # Errors
    ///
    /// Returns an error if cache metadata cannot be read/written or the cache
    /// directory cannot be prepared.
    pub fn prepare<T: AsRef<ShaderCacheInputs>>(inputs: T) -> Result<Self, crate::EngineError> {
        let inputs = inputs.as_ref();
        let next_manifest = json!({
            "project_mtime": modified_timestamp(&inputs.project_json_path, "project.json")?,
            "pkg_mtime": modified_timestamp(&inputs.scene_pkg_path, "scene.pkg")?,
            "property_override_json": inputs.property_override_json,
        });
        let scene_cache_path = inputs.cache_root.join(&inputs.scene_id);
        let manifest_path = scene_cache_path.join("manifest.json");

        let mut purge = inputs.force_refresh || !manifest_path.exists();
        if !purge {
            let current_manifest = fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|content| serde_json::from_str::<Value>(&content).ok());
            purge = current_manifest.as_ref() != Some(&next_manifest);
        }

        if purge && scene_cache_path.exists() {
            fs::remove_dir_all(&scene_cache_path).map_err(|error| {
                crate::EngineError::Render(format!("failed to clear scene shader cache: {error}"))
            })?;
        }

        fs::create_dir_all(&scene_cache_path).map_err(|error| {
            crate::EngineError::Render(format!(
                "failed to create scene shader cache directory: {error}"
            ))
        })?;
        let manifest_json = serde_json::to_string_pretty(&next_manifest).map_err(|error| {
            crate::EngineError::Render(format!("failed to encode shader cache manifest: {error}"))
        })?;
        fs::write(&manifest_path, manifest_json).map_err(|error| {
            crate::EngineError::Render(format!("failed to write shader cache manifest: {error}"))
        })?;

        Ok(Self {
            scene_cache_path,
            purged_cache: purge,
        })
    }

    /// Directory OWE should use for this scene's shader cache.
    #[must_use]
    pub fn scene_cache_path(&self) -> &Path {
        &self.scene_cache_path
    }

    /// Whether an existing cache directory was removed during preparation.
    #[must_use]
    pub fn purged_cache(&self) -> bool {
        self.purged_cache
    }
}

fn modified_timestamp(path: &Path, label: &str) -> Result<String, crate::EngineError> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| crate::EngineError::Render(format!("failed to stat {label}: {error}")))?;

    let duration = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(format!(
        "{}.{:09}",
        duration.as_secs(),
        duration.subsec_nanos()
    ))
}
