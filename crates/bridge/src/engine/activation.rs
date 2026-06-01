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
    config::{AppConfig, MonitorCfg, MonitorRender, MonitorSettingsCfg, WallpaperConfig},
    display::{DisplayDescExt, DisplaySelectorExt, DisplaySnapshotExt},
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

            let Some(display) = monitor.resolve_display(self.displays) else {
                continue;
            };
            if used_displays
                .iter()
                .any(|used| used.same_physical_display(&display))
            {
                continue;
            }

            let mut scene =
                self.scene_for_monitor(display.clone(), wallpaper_id, wallpaper, monitor)?;
            scene.horizontal_flip = self.monitor_horizontal_flip(monitor);
            used_displays.push(display);
            scenes.push(scene);
        }

        for monitor in self
            .app_config
            .monitors
            .iter()
            .filter(|monitor| monitor.enabled && monitor.mode.eq_ignore_ascii_case("mirror"))
        {
            let Some(display) = monitor.resolve_display(self.displays) else {
                continue;
            };
            if used_displays
                .iter()
                .any(|used| used.same_physical_display(&display))
            {
                continue;
            }
            let Some(target) = monitor.mirror_target.as_ref() else {
                continue;
            };
            let Some(source_display_id) = target
                .to_selector()
                .resolve_display(self.displays)
                .map(|entry| entry.desc.display_id)
            else {
                continue;
            };
            let Some(source_scene) = scenes
                .iter()
                .find(|scene| scene.display.display_id == source_display_id)
                .cloned()
            else {
                continue;
            };
            let settings = self
                .app_config
                .monitor_settings
                .iter()
                .find(|settings| settings.selector == monitor.selector)
                .cloned()
                .unwrap_or_else(|| MonitorSettingsCfg {
                    selector: monitor.selector.clone(),
                    ..MonitorSettingsCfg::default()
                });
            let audio_volume =
                AudioVolume::try_from(settings.volume).map_err(|error| BridgeError::Error {
                    kind: BridgeErrorKind::Engine,
                    message: EngineError::InvalidInput(error.to_string()).to_string(),
                })?;
            let mut scene = source_scene;
            scene.display = display.clone();
            scene.scaling_mode = settings.parse_scaling_mode();
            scene.scaling_factor = settings.scaling_factor;
            scene.horizontal_flip = settings.horizontal_flip;
            scene.fps = scene
                .display
                .refresh_rate_hz
                .max(1)
                .min(settings.target_fps.max(1));
            scene.audio_volume = audio_volume;
            scene.audio_muted = settings.muted;
            scene.validate().map_err(|error| BridgeError::Error {
                kind: BridgeErrorKind::Engine,
                message: error.to_string(),
            })?;
            used_displays.push(display);
            scenes.push(scene);
        }

        Ok(scenes)
    }

    fn scene_for_monitor(
        &self,
        display: DisplayDesc,
        wallpaper_id: &str,
        wallpaper: &WallpaperConfig,
        monitor: &MonitorCfg,
    ) -> Result<SceneDesc, BridgeError> {
        SceneDescBuilder::build_from_wallpaper_config(SceneBuildContext {
            display,
            workshop_id: wallpaper_id,
            wallpaper,
            monitor,
            displays: self.displays,
            paused: self.paused,
            paths: self.paths,
            force_shader_refresh: self.force_shader_refresh,
        })
    }

    fn monitor_horizontal_flip(&self, monitor: &MonitorCfg) -> bool {
        self.app_config
            .monitor_settings
            .iter()
            .find(|settings| settings.selector == monitor.selector)
            .is_some_and(|settings| settings.horizontal_flip)
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

trait MonitorCfgActivationExt {
    fn resolve_display(&self, displays: &[DisplaySnapshotEntry]) -> Option<DisplayDesc>;
}

impl MonitorCfgActivationExt for MonitorCfg {
    fn resolve_display(&self, displays: &[DisplaySnapshotEntry]) -> Option<DisplayDesc> {
        self.selector
            .to_selector()
            .resolve_display(displays)
            .map(|entry| entry.desc.clone())
    }
}

pub trait SceneDescBuilderExt {
    fn build_from_wallpaper_config(
        context: SceneBuildContext<'_>,
    ) -> Result<SceneDesc, BridgeError>
    where
        Self: Sized;
}

impl SceneDescBuilderExt for SceneDescBuilder {
    fn build_from_wallpaper_config(
        context: SceneBuildContext<'_>,
    ) -> Result<SceneDesc, BridgeError> {
        let project_json = context
            .paths
            .steam_workshop_root()
            .join(context.workshop_id)
            .join("project.json");
        let assets_path = context.paths.assets_root();
        let audio_volume =
            AudioVolume::try_from(context.wallpaper.audio.volume).map_err(|error| {
                BridgeError::Error {
                    kind: BridgeErrorKind::Engine,
                    message: EngineError::InvalidInput(error.to_string()).to_string(),
                }
            })?;
        let render_override = RenderOverrideResolver {
            wallpaper: context.wallpaper,
            monitor: context.monitor,
            display: &context.display,
            displays: context.displays,
        }
        .resolve();
        let fps = render_override.map_or(60, |render| render.fps);
        let max_fps = context.display.refresh_rate_hz.max(1);
        let scaling_mode = render_override
            .map(crate::config::wallpaper::MonitorRender::parse_scaling_mode)
            .unwrap_or_default();
        let scaling_factor = render_override.map_or(1.0, |render| render.scaling_factor);
        let property_override_json = if !context.wallpaper.r#type.eq_ignore_ascii_case("scene")
            || context.wallpaper.property_overrides.is_empty()
        {
            None
        } else {
            let overrides = context
                .wallpaper
                .property_overrides
                .iter()
                .map(|(id, value)| (id.clone(), PropertyValue::from_json(value)))
                .collect::<BTreeMap<_, _>>();

            Some(overrides.to_override_json())
        };
        let mut builder = SceneTemplate::builder(project_json.to_string_lossy())
            .assets_path(assets_path.to_string_lossy())
            .fps(fps.max(1).min(max_fps))
            .paused(context.paused)
            .scaling_mode(scaling_mode)
            .scaling_factor(scaling_factor)
            .audio_response_enabled(context.wallpaper.audio.response_enabled)
            .audio_volume(audio_volume.into())
            .audio_muted(context.wallpaper.audio.muted)
            .shader_cache_path(context.paths.shader_cache_root().to_string_lossy())
            .force_shader_refresh(context.force_shader_refresh);

        if let Some(json) = property_override_json {
            builder = builder.property_override_json(json);
        }

        builder
            .build()
            .map(|template| template.for_display(context.display))
            .map_err(|error| BridgeError::Error {
                kind: BridgeErrorKind::Engine,
                message: error.to_string(),
            })
    }
}

pub struct SceneBuildContext<'a> {
    display: DisplayDesc,
    workshop_id: &'a str,
    wallpaper: &'a WallpaperConfig,
    monitor: &'a MonitorCfg,
    displays: &'a [DisplaySnapshotEntry],
    paused: bool,
    paths: &'a BridgePaths,
    force_shader_refresh: bool,
}

struct RenderOverrideResolver<'a> {
    wallpaper: &'a WallpaperConfig,
    monitor: &'a MonitorCfg,
    display: &'a DisplayDesc,
    displays: &'a [DisplaySnapshotEntry],
}

impl<'a> RenderOverrideResolver<'a> {
    fn resolve(&self) -> Option<&'a MonitorRender> {
        self.wallpaper
            .monitors
            .iter()
            .find(|render| render.selector == self.monitor.selector)
            .or_else(|| {
                self.wallpaper
                    .monitors
                    .iter()
                    .find(|render| self.matches(render))
            })
    }

    fn matches(&self, render: &MonitorRender) -> bool {
        let Some(display_snapshot) = self.display_snapshot() else {
            return false;
        };

        if render.selector == crate::config::SerializedSelector::Primary {
            return self
                .displays
                .first()
                .is_some_and(|primary| display_snapshot.matches_primary(primary));
        }

        let render_selector = render.selector.to_selector();
        render_selector.matches_display(display_snapshot)
    }

    fn display_snapshot(&self) -> Option<&'a DisplaySnapshotEntry> {
        self.displays
            .iter()
            .find(|display| display.desc.same_physical_display(self.display))
    }
}
