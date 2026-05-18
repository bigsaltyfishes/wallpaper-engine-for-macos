use wallpaper_core::{DisplaySnapshotEntry, WallpaperAssignment, project::ScalingMode};

use crate::{
    actor::state::BridgeActorState,
    api::{
        BridgeDisplayConfigRow, BridgeDisplayMode, BridgeDisplaySettingsRow, BridgeError,
        BridgeMonitorInfoRow, BridgeMonitorInformationSnapshot, BridgePropertyDescriptor,
        BridgePropertyKind, BridgePropertyValue, BridgeScalingMode, BridgeSettingsSnapshot,
        BridgeSliderMetadata, BridgeWallpaperOptionsSnapshot,
    },
    config::SerializedSelector,
    display::{DisplayLabelExt, DisplaySnapshotExt},
    login::LaunchAtLoginStatus,
    project::PropertyMetadata,
};

const MIRROR_DISPLAY_MODE: &str = "mirror";

impl BridgeActorState {
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
    pub fn options(
        &self,
        displays: &[DisplaySnapshotEntry],
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperOptionsSnapshot, BridgeError> {
        let entry = self
            .library
            .iter()
            .find(|entry| entry.id == wallpaper_id)
            .ok_or_else(|| {
                BridgeError::invalid_input(format!("unknown wallpaper id {wallpaper_id}"))
            })?;

        let draft = self.wallpaper_draft(&wallpaper_id)?;
        let config = draft.current();
        let properties = self
            .project_models
            .get(&wallpaper_id)
            .map(|model| {
                let overrides = model.override_values(&config.property_overrides);
                model
                    .properties
                    .iter()
                    .map(|property| {
                        let value = property.effective_value(&overrides);
                        let default_value = property.default_value();
                        let dirty = !property.value_is_default(&value);
                        let slider = match &property.metadata {
                            PropertyMetadata::Slider {
                                min,
                                max,
                                step,
                                precision,
                                ..
                            } => Some(BridgeSliderMetadata {
                                min: *min,
                                max: *max,
                                step: *step,
                                precision: *precision,
                            }),
                            _ => None,
                        };

                        BridgePropertyDescriptor {
                            id: property.id.clone(),
                            kind: BridgePropertyKind::from(&property.kind),
                            label_html: property.label_html.clone(),
                            value: BridgePropertyValue::from(value),
                            default_value: BridgePropertyValue::from(default_value),
                            slider,
                            dirty,
                            can_restore_defaults: dirty,
                            enabled: true,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let app_config = self.app_config.normalized(displays);
        let display_configurations = app_config
            .monitor_rows(displays)
            .into_iter()
            .filter(|row| {
                row.connected && row.config.enabled && row.config.mode != MIRROR_DISPLAY_MODE
            })
            .filter_map(|row| {
                let display = row.display_index.and_then(|index| displays.get(index))?;
                let selector = &row.selector;
                let primary = *selector == SerializedSelector::Primary;
                let render = config
                    .monitors
                    .iter()
                    .find(|render| render.selector == *selector);
                let default_render = crate::config::MonitorRender {
                    selector: selector.clone(),
                    ..crate::config::MonitorRender::default()
                };
                let render = render.unwrap_or(&default_render);
                let max_fps = display.desc.refresh_rate_hz.max(1);
                let render_dirty = config
                    .monitors
                    .iter()
                    .find(|candidate| candidate.selector == *selector)
                    .is_some_and(|candidate| candidate != &default_render);

                Some(BridgeDisplayConfigRow {
                    display_id: selector.id(),
                    title: display.title_with_role(primary),
                    enabled: draft.display_enabled(selector),
                    scaling_mode: match render.parse_scaling_mode() {
                        ScalingMode::None => BridgeScalingMode::None,
                        ScalingMode::Stretch => BridgeScalingMode::Stretch,
                        ScalingMode::Fit => BridgeScalingMode::Match,
                        ScalingMode::Fill => BridgeScalingMode::Fill,
                    },
                    scaling_factor: render.scaling_factor,
                    target_fps: render.fps.min(max_fps),
                    max_fps,
                    dirty: render_dirty || draft.display_enabled_dirty(selector),
                    can_restore_defaults: render != &default_render
                        || draft.display_enabled_dirty(selector),
                })
            })
            .collect();

        Ok(BridgeWallpaperOptionsSnapshot {
            wallpaper_id: entry.id.clone(),
            title: entry.title.clone(),
            kind: entry.kind,
            supported: entry.supported,
            dirty: draft.is_dirty(),
            properties,
            display_configurations,
            audio_response_enabled: config.audio.response_enabled,
            muted: config.audio.muted,
            volume: config.audio.volume,
        })
    }

    pub fn monitor_info(
        &self,
        displays: &[DisplaySnapshotEntry],
    ) -> BridgeMonitorInformationSnapshot {
        let app_config = self.app_config.normalized(displays);
        let rows = app_config
            .monitor_rows(displays)
            .into_iter()
            .filter(|row| {
                row.connected
                    && row.config.enabled
                    && row.config.mode != MIRROR_DISPLAY_MODE
                    && row.config.wallpaper.is_some()
            })
            .filter_map(|row| {
                let display = row.display_index.and_then(|index| displays.get(index))?;
                let wallpaper_id = row.config.wallpaper.as_ref()?;
                let wallpaper_title = self
                    .library
                    .iter()
                    .find(|entry| entry.id == *wallpaper_id)
                    .map_or_else(|| wallpaper_id.clone(), |entry| entry.title.clone());
                let render = self.wallpaper_configs.get(wallpaper_id).and_then(|config| {
                    config
                        .monitors
                        .iter()
                        .find(|render| render.selector == row.selector)
                });
                let default_render = crate::config::MonitorRender {
                    selector: row.selector.clone(),
                    ..crate::config::MonitorRender::default()
                };
                let render = render.unwrap_or(&default_render);
                let primary = row.selector == SerializedSelector::Primary;
                let role = if primary { "Primary" } else { "Secondary" };
                let suffix = format!("({} - {role} - {wallpaper_id})", display.desc.display_id);
                let title = display.title_with_suffix(&suffix);
                let scaling_mode = match render.parse_scaling_mode() {
                    ScalingMode::None => "None",
                    ScalingMode::Stretch => "Stretch",
                    ScalingMode::Fit => "Match",
                    ScalingMode::Fill => "Fill",
                };

                Some(BridgeMonitorInfoRow {
                    display_id: row.selector.id(),
                    title,
                    wallpaper_id: wallpaper_id.clone(),
                    wallpaper_title,
                    scaling_mode: scaling_mode.to_string(),
                    target_fps: render
                        .fps
                        .min(display.desc.refresh_rate_hz.max(1))
                        .to_string(),
                    audio_response: self
                        .wallpaper_configs
                        .get(wallpaper_id)
                        .is_some_and(|config| config.audio.response_enabled),
                })
            })
            .collect();

        BridgeMonitorInformationSnapshot { rows }
    }

    pub fn settings(
        &self,
        displays: &[DisplaySnapshotEntry],
        launch_at_login: LaunchAtLoginStatus,
    ) -> BridgeSettingsSnapshot {
        let app_config = self.app_config.normalized(displays);
        let rows = if displays.is_empty() {
            self.display_settings.values().cloned().collect()
        } else {
            let rows = app_config.monitor_rows(displays);
            rows.iter()
                .filter_map(|row| {
                    let entry = row.display_index.and_then(|index| displays.get(index))?;
                    let configured = app_config
                        .monitors
                        .iter()
                        .any(|monitor| monitor.selector == row.selector);
                    let primary = row.selector == SerializedSelector::Primary;
                    let mode = if primary || !row.config.mode.eq_ignore_ascii_case("mirror") {
                        BridgeDisplayMode::Standalone
                    } else {
                        BridgeDisplayMode::Mirror
                    };
                    let mode = if configured {
                        mode
                    } else {
                        match entry.assignment.as_ref() {
                            Some(WallpaperAssignment::Mirror(_)) => BridgeDisplayMode::Mirror,
                            _ => BridgeDisplayMode::Standalone,
                        }
                    };
                    let selected_mirror_target = if mode == BridgeDisplayMode::Mirror && configured
                    {
                        row.config
                            .mirror_target
                            .as_ref()
                            .and_then(|selector| selector.mirror_target_id(entry, displays))
                    } else if mode == BridgeDisplayMode::Mirror {
                        match entry.assignment.as_ref()? {
                            WallpaperAssignment::Mirror(selector) => {
                                SerializedSelector::from_selector(selector)
                                    .mirror_target_id(entry, displays)
                            }
                            WallpaperAssignment::Direct(_) => None,
                        }
                    } else {
                        None
                    };

                    Some(BridgeDisplaySettingsRow {
                        display_id: row.selector.id(),
                        title: entry.title_with_role(primary),
                        enabled: primary || row.config.enabled,
                        mode,
                        mirror_targets: rows
                            .iter()
                            .filter(|candidate| {
                                candidate.connected
                                    && candidate.config.enabled
                                    && candidate.config.mode != MIRROR_DISPLAY_MODE
                                    && candidate.selector != row.selector
                            })
                            .filter_map(|candidate| {
                                candidate
                                    .display_index
                                    .and_then(|index| displays.get(index))
                            })
                            .map(|display| display.config_selector(displays).id())
                            .collect(),
                        selected_mirror_target,
                    })
                })
                .collect()
        };
        let version = env!("CARGO_PKG_VERSION").to_string();

        BridgeSettingsSnapshot {
            displays: rows,
            launch_at_login_available: matches!(
                launch_at_login,
                LaunchAtLoginStatus::Available { .. }
            ),
            launch_at_login_enabled: match launch_at_login {
                LaunchAtLoginStatus::Available { enabled } => enabled,
                LaunchAtLoginStatus::Unavailable => false,
            },
            app_version: version.clone(),
            git_sha: option_env!("GIT_SHA").unwrap_or("Unknown").to_string(),
            bridge_version: version.clone(),
            core_version: version,
        }
    }
}
