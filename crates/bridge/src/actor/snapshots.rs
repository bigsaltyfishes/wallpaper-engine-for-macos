use std::{fs, path::Path};

use wallpaper_core::{DisplaySnapshotEntry, WallpaperAssignment, project::ScalingMode};

use crate::{
    actor::state::BridgeActorState,
    api::{
        BridgeDisplayConfigRow, BridgeDisplayMode, BridgeDisplaySettingsRow, BridgeError,
        BridgeMonitorInfoRow, BridgeMonitorInformationSnapshot, BridgePropertyDescriptor,
        BridgePropertyKind, BridgePropertyValue, BridgeScalingMode, BridgeSettingsSnapshot,
        BridgeSliderMetadata, BridgeStorageStatus, BridgeWallpaperOptionsSnapshot,
        bridge_log_status,
    },
    config::SerializedSelector,
    display::{DisplayLabelExt, DisplaySelectorExt, DisplaySnapshotExt},
    logging::{ApplicationLogger, LogStatus},
    login::LaunchAtLoginStatus,
    paths::BridgePaths,
    project::PropertyMetadata,
};

const MIRROR_DISPLAY_MODE: &str = "mirror";
const UNKNOWN_GIT_SHA: &str = "Unknown";
const SHADER_PIPELINE_VERSION: &str = "0.1.0";

fn directory_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }

    fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| directory_size(&entry.path()))
        .sum()
}

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
        let active_enabled_displays = self.enabled_selectors(&wallpaper_id);
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
                    enabled: draft.effective_display_enabled(selector, &active_enabled_displays),
                    scaling_mode: match render.parse_scaling_mode() {
                        ScalingMode::None => BridgeScalingMode::None,
                        ScalingMode::Stretch => BridgeScalingMode::Stretch,
                        ScalingMode::Fit => BridgeScalingMode::Match,
                        ScalingMode::Fill => BridgeScalingMode::Fill,
                    },
                    scaling_factor: render.scaling_factor,
                    target_fps: render.fps.min(max_fps),
                    max_fps,
                    muted: config.audio.muted,
                    volume: config.audio.volume,
                    dirty: render_dirty || draft.display_dirty(selector, &active_enabled_displays),
                    can_restore_defaults: render != &default_render
                        || draft.display_dirty(selector, &active_enabled_displays),
                })
            })
            .collect();

        Ok(BridgeWallpaperOptionsSnapshot {
            wallpaper_id: entry.id.clone(),
            title: entry.title.clone(),
            kind: entry.kind,
            supported: entry.supported,
            dirty: draft.is_dirty(&active_enabled_displays),
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
                    && (row.config.wallpaper.is_some()
                        || row.config.mode == MIRROR_DISPLAY_MODE
                            && row.config.mirror_target.is_some())
            })
            .filter_map(|row| {
                let display = row.display_index.and_then(|index| displays.get(index))?;
                let mirror_target = if row.config.mode == MIRROR_DISPLAY_MODE {
                    row.config.mirror_target.as_ref()
                } else {
                    None
                };
                let target_display = mirror_target
                    .and_then(|selector| selector.to_selector().resolve_display(displays));
                let target_config_selector =
                    target_display.map(|display| display.config_selector(displays));
                let target_row = target_config_selector.as_ref().and_then(|selector| {
                    app_config
                        .monitor_rows(displays)
                        .into_iter()
                        .find(|candidate| candidate.selector == *selector)
                });
                let wallpaper_id = row.config.wallpaper.as_ref().or_else(|| {
                    target_row
                        .as_ref()
                        .and_then(|target| target.config.wallpaper.as_ref())
                })?;
                let wallpaper_title = self
                    .library
                    .iter()
                    .find(|entry| entry.id == *wallpaper_id)
                    .map_or_else(|| wallpaper_id.clone(), |entry| entry.title.clone());
                let render_selector = target_config_selector.as_ref().unwrap_or(&row.selector);
                let render = self.wallpaper_configs.get(wallpaper_id).and_then(|config| {
                    config
                        .monitors
                        .iter()
                        .find(|render| render.selector == *render_selector)
                });
                let default_render = crate::config::MonitorRender {
                    selector: render_selector.clone(),
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
                    mirror_target_display_id: target_config_selector
                        .as_ref()
                        .map(SerializedSelector::id),
                    mirror_target_title: target_display.map(|display| {
                        let primary =
                            target_config_selector.as_ref() == Some(&SerializedSelector::Primary);
                        display.title_with_role(primary)
                    }),
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

    #[allow(clippy::too_many_lines)]
    pub fn settings(
        &self,
        displays: &[DisplaySnapshotEntry],
        launch_at_login: LaunchAtLoginStatus,
        paths: &BridgePaths,
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
                    let settings = app_config
                        .monitor_settings
                        .iter()
                        .find(|settings| settings.selector == row.selector)
                        .cloned()
                        .unwrap_or_else(|| crate::config::MonitorSettingsCfg {
                            selector: row.selector.clone(),
                            ..crate::config::MonitorSettingsCfg::default()
                        });
                    let scaling_mode = BridgeScalingMode::from(settings.parse_scaling_mode());
                    let max_fps = entry.desc.refresh_rate_hz.max(1);

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
                        scaling_mode,
                        scaling_factor: settings.scaling_factor,
                        target_fps: settings.target_fps.min(max_fps),
                        max_fps,
                        muted: settings.muted,
                        volume: settings.volume,
                    })
                })
                .collect()
        };
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
            pause_on_battery_power: self.app_config.power.pause_on_battery_power,
            git_sha: match option_env!("GIT_SHORT_COMMIT").unwrap_or(crate::build::SHORT_COMMIT) {
                value if value.trim().is_empty() => UNKNOWN_GIT_SHA.to_string(),
                value => value.to_string(),
            },
            bridge_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: wallpaper_core::VERSION.to_string(),
            web_version: wallpaper_web::VERSION.to_string(),
            shader_pipeline_version: SHADER_PIPELINE_VERSION.to_string(),
            storage: BridgeStorageStatus {
                shader_cache_size_bytes: directory_size(&paths.shader_cache_root()),
                logs: ApplicationLogger::status().map_or_else(
                    || {
                        bridge_log_status(LogStatus {
                            logs_root: paths.logs_root(),
                            active_session: String::new(),
                            active_file: paths.logs_root().join("0.log"),
                            active_file_size_bytes: 0,
                        })
                    },
                    bridge_log_status,
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::directory_size;
    use crate::{login::LaunchAtLoginStatus, paths::BridgePaths};

    #[test]
    fn directory_size_sums_nested_files() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.path().join("a.bin"), [1, 2, 3]).unwrap();
        fs::write(nested.join("b.bin"), [4, 5]).unwrap();

        assert_eq!(directory_size(root.path()), 5);
    }

    #[test]
    fn settings_snapshot_reports_storage_status() {
        let root = tempfile::tempdir().unwrap();
        let paths = BridgePaths::for_home(root.path());
        fs::create_dir_all(paths.shader_cache_root()).unwrap();
        fs::write(paths.shader_cache_root().join("shader.bin"), [1, 2, 3, 4]).unwrap();

        let snapshot = crate::actor::state::BridgeActorState::default().settings(
            &[],
            LaunchAtLoginStatus::Unavailable,
            &paths,
        );

        assert_eq!(snapshot.storage.shader_cache_size_bytes, 4);
        assert_eq!(
            snapshot.storage.logs.logs_root,
            paths.logs_root().to_string_lossy()
        );
    }
}
