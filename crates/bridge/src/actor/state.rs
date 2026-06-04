pub mod drafts {
    pub use crate::state::drafts::*;
}

use std::collections::BTreeMap;

use drafts::WallpaperOptionsDraft;
use wallpaper_core::project::{SceneDesc, WallpaperProjectType};

use crate::{
    api::{
        BridgeDisplaySettingsRow, BridgeError, BridgePlaybackState, BridgeWallpaperEntry,
        BridgeWallpaperKind,
    },
    config::{AppConfig, SerializedSelector, WallpaperConfig},
    project::ProjectModel,
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct BridgeActorState {
    pub playback_state: BridgePlaybackState,
    pub selected_wallpaper_id: Option<String>,
    pub active_wallpaper_ids: Vec<String>,
    pub errors: Vec<String>,
    pub library: Vec<BridgeWallpaperEntry>,
    pub app_config: AppConfig,
    pub wallpaper_configs: BTreeMap<String, WallpaperConfig>,
    pub wallpaper_drafts: BTreeMap<String, WallpaperOptionsDraft>,
    pub project_models: BTreeMap<String, ProjectModel>,
    pub display_settings: BTreeMap<String, BridgeDisplaySettingsRow>,
    pub power_source: crate::power::PowerSource,
    pub auto_paused_for_battery: bool,
    pub battery_pause_suppressed: bool,
    pub startup_power_sample_received: bool,
    pub initial_frame_ready: bool,
    pub pending_battery_pause_after_initial_frame: bool,
    pub filter_scene: bool,
    pub filter_video: bool,
    pub filter_webpage: bool,
    pub filter_unknown: bool,
}

impl Default for BridgeActorState {
    fn default() -> Self {
        Self {
            playback_state: BridgePlaybackState::Playing,
            selected_wallpaper_id: None,
            active_wallpaper_ids: Vec::new(),
            errors: Vec::new(),
            library: Vec::new(),
            app_config: AppConfig::default(),
            wallpaper_configs: BTreeMap::new(),
            wallpaper_drafts: BTreeMap::new(),
            project_models: BTreeMap::new(),
            display_settings: BTreeMap::new(),
            power_source: crate::power::PowerSource::External,
            auto_paused_for_battery: false,
            battery_pause_suppressed: false,
            startup_power_sample_received: false,
            initial_frame_ready: false,
            pending_battery_pause_after_initial_frame: false,
            filter_scene: true,
            filter_video: true,
            filter_webpage: true,
            filter_unknown: true,
        }
    }
}

pub struct ApplyCandidates {
    pub app_config: AppConfig,
    pub wallpaper_config: WallpaperConfig,
    pub wallpaper_configs: BTreeMap<String, WallpaperConfig>,
    pub enabled_displays: Vec<SerializedSelector>,
    pub requires_reconcile: bool,
}

impl BridgeActorState {
    #[allow(clippy::single_call_fn)]
    pub fn from_app_config(app_config: AppConfig) -> Self {
        let active_wallpaper_ids = Self::active_ids(&app_config);
        Self {
            filter_scene: app_config.ui.filter.scene,
            filter_video: app_config.ui.filter.video,
            filter_webpage: app_config.ui.filter.web,
            filter_unknown: app_config.ui.filter.unknown,
            selected_wallpaper_id: app_config.general.last_selected_wallpaper.clone(),
            active_wallpaper_ids,
            app_config,
            ..Self::default()
        }
    }

    pub fn apply_startup_power_source(&mut self, source: crate::power::PowerSource) {
        self.power_source = source;
        self.startup_power_sample_received = true;
        self.pending_battery_pause_after_initial_frame = source
            == crate::power::PowerSource::Battery
            && self.app_config.power.pause_on_battery_power
            && !self.initial_frame_ready;
    }

    pub fn filter_enabled(&self, kind: BridgeWallpaperKind) -> bool {
        match kind {
            BridgeWallpaperKind::ProjectScene => self.filter_scene,
            BridgeWallpaperKind::Video => self.filter_video,
            BridgeWallpaperKind::Webpage => self.filter_webpage,
            BridgeWallpaperKind::Unknown => self.filter_unknown,
        }
    }

    pub fn ensure_wallpaper_exists(&self, wallpaper_id: &str) -> Result<(), BridgeError> {
        self.library
            .iter()
            .any(|entry| entry.id == wallpaper_id)
            .then_some(())
            .ok_or_else(|| {
                BridgeError::invalid_input(format!("unknown wallpaper id {wallpaper_id}"))
            })
    }

    pub fn project_model(&self, wallpaper_id: &str) -> Result<&ProjectModel, BridgeError> {
        self.ensure_wallpaper_exists(wallpaper_id)?;
        self.project_models.get(wallpaper_id).ok_or_else(|| {
            BridgeError::invalid_input(format!(
                "no project model loaded for wallpaper id {wallpaper_id}"
            ))
        })
    }

    pub fn wallpaper_draft(
        &self,
        wallpaper_id: &str,
    ) -> Result<WallpaperOptionsDraft, BridgeError> {
        self.ensure_wallpaper_exists(wallpaper_id)?;
        Ok(self
            .wallpaper_drafts
            .get(wallpaper_id)
            .cloned()
            .or_else(|| {
                self.wallpaper_configs
                    .get(wallpaper_id)
                    .cloned()
                    .map(|config| {
                        WallpaperOptionsDraft::from_committed_with_enabled_displays(
                            config,
                            self.enabled_selectors(wallpaper_id),
                        )
                    })
            })
            .unwrap_or_else(|| {
                WallpaperOptionsDraft::from_committed_with_enabled_displays(
                    self.default_wallpaper_config(wallpaper_id),
                    self.enabled_selectors(wallpaper_id),
                )
            }))
    }

    pub fn apply_candidates(
        &self,
        wallpaper_id: &str,
        primary_identity_selector: Option<&SerializedSelector>,
    ) -> Result<ApplyCandidates, BridgeError> {
        let draft = self.wallpaper_draft(wallpaper_id)?;
        let active_enabled_displays = self.enabled_selectors(wallpaper_id);
        let requires_reconcile = draft.requires_reconcile(&active_enabled_displays);
        let wallpaper_config = draft.current().clone();
        let mut wallpaper_config = wallpaper_config;
        let enabled_displays = draft.effective_enabled_displays(&active_enabled_displays);
        let mut app_config = self.app_config.clone();

        for monitor in &mut app_config.monitors {
            if monitor.wallpaper.as_deref() == Some(wallpaper_id) {
                monitor.wallpaper = None;
            }
        }

        for selector in &enabled_displays {
            let monitor = app_config.ensure_monitor(selector.clone());
            monitor.enabled = true;
            monitor.mode = "independent".to_string();
            monitor.wallpaper = Some(wallpaper_id.to_string());
        }

        if let Some(identity_selector) = primary_identity_selector {
            if let Some(primary_index) = app_config
                .monitors
                .iter()
                .position(|monitor| monitor.selector == SerializedSelector::Primary)
            {
                let mut identity_monitor = app_config.monitors[primary_index].clone();
                identity_monitor.selector = identity_selector.clone();
                match app_config
                    .monitors
                    .iter()
                    .position(|monitor| &monitor.selector == identity_selector)
                {
                    Some(identity_index) => app_config.monitors[identity_index] = identity_monitor,
                    None => app_config.monitors.push(identity_monitor),
                }
            }

            if let Some(primary_index) = wallpaper_config
                .monitors
                .iter()
                .position(|render| render.selector == SerializedSelector::Primary)
            {
                let mut identity_render = wallpaper_config.monitors[primary_index].clone();
                identity_render.selector = identity_selector.clone();
                match wallpaper_config
                    .monitors
                    .iter()
                    .position(|render| &render.selector == identity_selector)
                {
                    Some(identity_index) => {
                        wallpaper_config.monitors[identity_index] = identity_render;
                    }
                    None => wallpaper_config.monitors.push(identity_render),
                }
            }
        }

        let mut wallpaper_configs = self.wallpaper_configs.clone();
        wallpaper_configs.insert(wallpaper_id.to_string(), wallpaper_config.clone());

        Ok(ApplyCandidates {
            app_config,
            wallpaper_config,
            wallpaper_configs,
            enabled_displays,
            requires_reconcile,
        })
    }

    pub fn commit_apply_candidates(
        &mut self,
        wallpaper_id: String,
        candidates: ApplyCandidates,
        active_from_config: bool,
    ) -> Result<(), BridgeError> {
        self.ensure_wallpaper_exists(&wallpaper_id)?;
        self.app_config = candidates.app_config;
        self.wallpaper_configs = candidates.wallpaper_configs;
        if active_from_config {
            self.refresh_active_ids();
        }
        self.wallpaper_drafts.insert(
            wallpaper_id,
            WallpaperOptionsDraft::from_committed_with_enabled_displays(
                candidates.wallpaper_config,
                candidates.enabled_displays,
            ),
        );
        Ok(())
    }

    pub fn refresh_active_ids(&mut self) {
        self.active_wallpaper_ids = Self::active_ids(&self.app_config);
    }

    pub fn set_active_ids_from_scenes(&mut self, scenes: &[SceneDesc]) {
        let mut ids = scenes
            .iter()
            .filter_map(|scene| {
                let path = std::path::Path::new(&scene.scene_path);
                path.parent()?
                    .file_name()?
                    .to_str()
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string)
            })
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        self.active_wallpaper_ids = ids;
    }

    pub fn wallpaper_draft_mut(
        &mut self,
        wallpaper_id: &str,
    ) -> Result<&mut WallpaperOptionsDraft, BridgeError> {
        self.ensure_wallpaper_exists(wallpaper_id)?;
        if !self.wallpaper_drafts.contains_key(wallpaper_id) {
            let committed = self
                .wallpaper_configs
                .get(wallpaper_id)
                .cloned()
                .unwrap_or_else(|| {
                    let type_str = self
                        .project_models
                        .get(wallpaper_id)
                        .map(|model| match model.project_type {
                            WallpaperProjectType::Scene => "scene",
                            WallpaperProjectType::Video => "video",
                            WallpaperProjectType::Web => "web",
                            WallpaperProjectType::Unknown => "scene",
                        })
                        .unwrap_or("scene");
                    WallpaperConfig::new_for(wallpaper_id, type_str)
                });
            let enabled_displays = self.enabled_selectors(wallpaper_id);
            self.wallpaper_drafts.insert(
                wallpaper_id.to_string(),
                WallpaperOptionsDraft::from_committed_with_enabled_displays(
                    committed,
                    enabled_displays,
                ),
            );
        }

        Ok(self
            .wallpaper_drafts
            .get_mut(wallpaper_id)
            .expect("draft exists after insertion"))
    }

    pub fn enabled_selectors(&self, wallpaper_id: &str) -> Vec<SerializedSelector> {
        self.app_config
            .monitors
            .iter()
            .filter(|monitor| monitor.enabled && monitor.wallpaper.as_deref() == Some(wallpaper_id))
            .map(|monitor| monitor.selector.clone())
            .collect()
    }

    pub fn configured_ids(&self) -> Vec<String> {
        Self::active_ids(&self.app_config)
    }

    pub fn replace_library(&mut self, library: Vec<BridgeWallpaperEntry>) {
        self.library = library;
        if let Some(selected_wallpaper_id) = &self.selected_wallpaper_id
            && !self
                .library
                .iter()
                .any(|entry| entry.id == *selected_wallpaper_id)
        {
            self.selected_wallpaper_id = None;
            self.app_config.general.last_selected_wallpaper = None;
        }
    }

    pub fn rebase_drafts(&mut self) {
        let enabled_by_wallpaper = self
            .wallpaper_drafts
            .keys()
            .map(|wallpaper_id| (wallpaper_id.clone(), self.enabled_selectors(wallpaper_id)))
            .collect::<Vec<_>>();

        for (wallpaper_id, enabled_displays) in enabled_by_wallpaper {
            if let Some(draft) = self.wallpaper_drafts.get_mut(&wallpaper_id) {
                draft.rebase_enabled_displays(enabled_displays);
            }
        }
    }

    fn active_ids(app_config: &AppConfig) -> Vec<String> {
        let mut ids = app_config
            .monitors
            .iter()
            .filter(|monitor| monitor.enabled && monitor.mode != "mirror")
            .filter_map(|monitor| monitor.wallpaper.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids
    }
}
