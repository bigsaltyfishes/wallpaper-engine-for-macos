//! Translate bridge state into the `SceneDesc` list consumed by engine
//! reconciliation.

use std::collections::BTreeMap;

use wallpaper_core::{
    DisplayDesc, DisplaySelector, DisplaySnapshotEntry, EngineError, WallpaperAssignment,
    media::audio::AudioVolume,
    project::{SceneDesc, SceneDescBuilder, SceneTemplate},
};

use crate::{
    api::{BridgeError, BridgeErrorKind},
    config::{AppConfig, MonitorCfg, WallpaperConfig},
    display::{DisplayDescExt, DisplayIdentityExt, DisplaySelectorExt},
    paths::BridgePaths,
    project::{OverrideMapExt, PropertyValue},
};

pub struct ActivationInputs<'a> {
    pub app_config: &'a AppConfig,
    pub wallpapers: &'a BTreeMap<String, WallpaperConfig>,
    pub displays: &'a [DisplaySnapshotEntry],
    pub paused: bool,
    pub paths: &'a BridgePaths,
    pub force_shader_refresh: bool,
}

impl ActivationInputs<'_> {
    /// # Errors
    ///
    /// Returns an error when an active wallpaper config cannot be converted
    /// into a scene.
    pub fn build(self) -> Result<Vec<SceneDesc>, BridgeError> {
        let mut scenes = Vec::new();
        let mut used_displays: Vec<DisplayDesc> = Vec::new();
        let mut monitors = self.app_config.monitors.iter().collect::<Vec<_>>();
        monitors.sort_by_key(|monitor| {
            i32::from(monitor.selector != crate::config::SerializedSelector::Primary)
        });

        for monitor in monitors {
            if !monitor.enabled {
                continue;
            }

            let Some(wallpaper_id) = monitor.wallpaper.as_deref() else {
                continue;
            };

            let Some(wallpaper) = self.wallpapers.get(wallpaper_id) else {
                continue;
            };

            if monitor.mode == "mirror" {
                continue;
            }

            let Some(display) = monitor
                .selector
                .to_selector()
                .resolve_display(self.displays)
                .map(|entry| entry.desc.clone())
            else {
                continue;
            };
            if used_displays
                .iter()
                .any(|used| used.same_physical_display(&display))
            {
                continue;
            }

            let scene = SceneDescBuilder::build_from_wallpaper_config(
                display.clone(),
                wallpaper_id,
                wallpaper,
                monitor,
                self.paused,
                self.paths,
                self.force_shader_refresh,
            )?;
            used_displays.push(display);
            scenes.push(scene);
        }

        Ok(scenes)
    }
}

pub trait WallpaperAssignmentExt {
    fn build_mirror_assignments(
        app_config: &AppConfig,
        displays: &[DisplaySnapshotEntry],
    ) -> Vec<(DisplaySelector, WallpaperAssignment)>;
}

impl WallpaperAssignmentExt for WallpaperAssignment {
    fn build_mirror_assignments(
        app_config: &AppConfig,
        displays: &[DisplaySnapshotEntry],
    ) -> Vec<(DisplaySelector, WallpaperAssignment)> {
        let mut assignments = Vec::new();

        for monitor in &app_config.monitors {
            if !monitor.enabled || monitor.mode != "mirror" {
                continue;
            }
            if monitor
                .selector
                .to_selector()
                .resolve_display(displays)
                .is_none()
            {
                continue;
            }
            let Some(target) = monitor.mirror_target.as_ref() else {
                continue;
            };
            assignments.push((
                monitor.selector.to_selector(),
                WallpaperAssignment::Mirror(target.to_selector()),
            ));
        }

        assignments
    }
}

pub trait SceneDescBuilderExt {
    fn build_from_wallpaper_config(
        display: DisplayDesc,
        workshop_id: &str,
        wallpaper: &WallpaperConfig,
        monitor: &MonitorCfg,
        paused: bool,
        paths: &BridgePaths,
        force_shader_refresh: bool,
    ) -> Result<SceneDesc, BridgeError>
    where
        Self: Sized;
}

impl SceneDescBuilderExt for SceneDescBuilder {
    fn build_from_wallpaper_config(
        display: DisplayDesc,
        workshop_id: &str,
        wallpaper: &WallpaperConfig,
        monitor: &MonitorCfg,
        paused: bool,
        paths: &BridgePaths,
        force_shader_refresh: bool,
    ) -> Result<SceneDesc, BridgeError> {
        let project_json = paths
            .steam_workshop_root()
            .join(workshop_id)
            .join("project.json");
        let assets_path = paths.assets_root();
        let audio_volume =
            AudioVolume::try_from(wallpaper.audio.volume).map_err(|error| BridgeError::Error {
                kind: BridgeErrorKind::Engine,
                message: EngineError::InvalidInput(error.to_string()).to_string(),
            })?;
        let render_override = wallpaper
            .monitors
            .iter()
            .find(|render| render.selector == monitor.selector)
            .or_else(|| {
                if monitor.selector == crate::config::SerializedSelector::Primary {
                    let identity_selector = display.identity.has_stable_identity().then(|| {
                        crate::config::SerializedSelector::from_selector(
                            &DisplaySelector::Identity(display.identity.clone()),
                        )
                    })?;
                    wallpaper
                        .monitors
                        .iter()
                        .find(|render| render.selector == identity_selector)
                } else {
                    None
                }
            });
        let fps = render_override.map_or(60, |render| render.fps);
        let max_fps = display.refresh_rate_hz.max(1);
        let scaling_mode = render_override
            .map(crate::config::wallpaper::MonitorRender::parse_scaling_mode)
            .unwrap_or_default();
        let scaling_factor = render_override.map_or(1.0, |render| render.scaling_factor);
        let property_override_json = if !wallpaper.r#type.eq_ignore_ascii_case("scene")
            || wallpaper.property_overrides.is_empty()
        {
            None
        } else {
            let overrides = wallpaper
                .property_overrides
                .iter()
                .map(|(id, value)| (id.clone(), PropertyValue::from_json(value)))
                .collect::<BTreeMap<_, _>>();

            Some(overrides.to_override_json())
        };
        let mut builder = SceneTemplate::builder(project_json.to_string_lossy())
            .assets_path(assets_path.to_string_lossy())
            .fps(fps.max(1).min(max_fps))
            .paused(paused)
            .scaling_mode(scaling_mode)
            .scaling_factor(scaling_factor)
            .audio_response_enabled(wallpaper.audio.response_enabled)
            .audio_volume(audio_volume.into())
            .audio_muted(wallpaper.audio.muted)
            .shader_cache_path(paths.shader_cache_root().to_string_lossy())
            .force_shader_refresh(force_shader_refresh);

        if let Some(json) = property_override_json {
            builder = builder.property_override_json(json);
        }

        builder
            .build()
            .map(|template| template.for_display(display))
            .map_err(|error| BridgeError::Error {
                kind: BridgeErrorKind::Engine,
                message: error.to_string(),
            })
    }
}
