use wallpaper_core::project::ScalingMode;

use crate::{
    api::BridgeError,
    config::{MonitorRender, SerializedSelector, WallpaperConfig},
    project::{ProjectModel, PropertyValue},
};

#[derive(Clone, Debug)]
pub struct WallpaperOptionsDraft {
    committed: WallpaperConfig,
    current: WallpaperConfig,
    committed_enabled_displays: Vec<SerializedSelector>,
    current_enabled_displays: Vec<SerializedSelector>,
}

impl WallpaperOptionsDraft {
    #[must_use]
    pub fn from_committed(committed: WallpaperConfig) -> Self {
        Self::from_committed_with_enabled_displays(committed, Vec::new())
    }

    #[must_use]
    pub fn from_committed_with_enabled_displays(
        committed: WallpaperConfig,
        enabled_displays: Vec<SerializedSelector>,
    ) -> Self {
        Self {
            current: committed.clone(),
            committed,
            committed_enabled_displays: enabled_displays.clone(),
            current_enabled_displays: enabled_displays,
        }
    }

    #[must_use]
    pub fn current(&self) -> &WallpaperConfig {
        &self.current
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.current != self.committed
            || !same_selector_set(
                &self.current_enabled_displays,
                &self.committed_enabled_displays,
            )
    }

    #[must_use]
    pub fn requires_reconcile(&self) -> bool {
        !same_selector_set(
            &self.current_enabled_displays,
            &self.committed_enabled_displays,
        ) || {
            let mut current = self.current.clone();
            let mut committed = self.committed.clone();
            current.audio = committed.audio.clone();
            for current_render in &mut current.monitors {
                if let Some(committed_render) = committed
                    .monitors
                    .iter_mut()
                    .find(|render| render.selector == current_render.selector)
                {
                    current_render.scaling_factor = committed_render.scaling_factor;
                } else if current_render.scaling_mode == MonitorRender::default().scaling_mode
                    && current_render.fps == MonitorRender::default().fps
                {
                    current_render.scaling_factor = MonitorRender::default().scaling_factor;
                    committed.monitors.push(current_render.clone());
                }
            }
            current != committed
        }
    }

    /// # Errors
    ///
    /// Returns an error when `volume` is outside the inclusive `0.0..=1.0`
    /// range.
    pub fn set_volume(&mut self, volume: f32) -> Result<(), BridgeError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(BridgeError::invalid_input(
                "wallpaper volume must be between 0 and 1",
            ));
        }

        self.current.audio.volume = volume;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when `volume` is outside the inclusive `0.0..=1.0`
    /// range.
    pub fn set_volume_immediate(&mut self, volume: f32) -> Result<WallpaperConfig, BridgeError> {
        self.set_volume(volume)?;
        self.committed.audio.volume = volume;
        Ok(self.committed.clone())
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.current.audio.muted = muted;
    }

    #[must_use]
    pub fn set_muted_immediate(&mut self, muted: bool) -> WallpaperConfig {
        self.set_muted(muted);
        self.committed.audio.muted = muted;
        self.committed.clone()
    }

    pub fn set_audio_response_enabled(&mut self, enabled: bool) {
        self.current.audio.response_enabled = enabled;
    }

    #[must_use]
    pub fn set_audio_response_enabled_immediate(&mut self, enabled: bool) -> WallpaperConfig {
        self.set_audio_response_enabled(enabled);
        self.committed.audio.response_enabled = enabled;
        self.committed.clone()
    }

    pub fn set_display_enabled(&mut self, selector: SerializedSelector, enabled: bool) {
        if enabled {
            if !self.current_enabled_displays.contains(&selector) {
                self.current_enabled_displays.push(selector);
            }
        } else {
            self.current_enabled_displays
                .retain(|candidate| candidate != &selector);
        }
    }

    #[must_use]
    pub fn display_enabled(&self, selector: &SerializedSelector) -> bool {
        self.current_enabled_displays.contains(selector)
    }

    #[must_use]
    pub fn display_enabled_dirty(&self, selector: &SerializedSelector) -> bool {
        self.current_enabled_displays.contains(selector)
            != self.committed_enabled_displays.contains(selector)
    }

    #[must_use]
    pub fn enabled_displays(&self) -> &[SerializedSelector] {
        &self.current_enabled_displays
    }

    pub fn rebase_enabled_displays(&mut self, enabled_displays: Vec<SerializedSelector>) {
        self.committed_enabled_displays
            .clone_from(&enabled_displays);
        self.current_enabled_displays = enabled_displays;
    }

    pub fn set_display_render_enabled(&mut self, selector: SerializedSelector, enabled: bool) {
        if enabled {
            let _ = self.ensure_monitor_render(selector);
        } else {
            self.current
                .monitors
                .retain(|render| render.selector != selector);
        }
    }

    pub fn set_scaling_mode(&mut self, selector: SerializedSelector, mode: ScalingMode) {
        self.ensure_monitor_render(selector).scaling_mode = mode.to_string();
    }

    #[must_use]
    pub fn set_scaling_mode_immediate(
        &mut self,
        selector: SerializedSelector,
        mode: ScalingMode,
    ) -> WallpaperConfig {
        self.set_scaling_mode(selector.clone(), mode);
        self.committed
            .monitors
            .iter_mut()
            .find(|render| render.selector == selector)
            .map(|render| render.scaling_mode = mode.to_string())
            .unwrap_or_else(|| {
                self.committed.monitors.push(MonitorRender {
                    selector,
                    scaling_mode: mode.to_string(),
                    ..MonitorRender::default()
                });
            });
        self.committed.clone()
    }

    /// # Errors
    ///
    /// Returns an error when `factor` is not finite and greater than 0.
    pub fn set_scaling_factor(
        &mut self,
        selector: SerializedSelector,
        factor: f64,
    ) -> Result<(), BridgeError> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(BridgeError::invalid_input(
                "scaling factor must be finite and greater than 0",
            ));
        }

        self.ensure_monitor_render(selector).scaling_factor = factor;
        Ok(())
    }

    pub fn set_target_fps(&mut self, selector: SerializedSelector, fps: u32, max_fps: u32) {
        self.ensure_monitor_render(selector).fps = fps.min(max_fps.max(1));
    }

    #[must_use]
    pub fn set_target_fps_immediate(
        &mut self,
        selector: SerializedSelector,
        fps: u32,
        max_fps: u32,
    ) -> WallpaperConfig {
        self.set_target_fps(selector.clone(), fps, max_fps);
        let fps = fps.min(max_fps.max(1));
        self.committed
            .monitors
            .iter_mut()
            .find(|render| render.selector == selector)
            .map(|render| render.fps = fps)
            .unwrap_or_else(|| {
                self.committed.monitors.push(MonitorRender {
                    selector,
                    fps,
                    ..MonitorRender::default()
                });
            });
        self.committed.clone()
    }

    pub fn edit_property(&mut self, model: &ProjectModel, id: &str, value: PropertyValue) {
        model.edit_overrides(&mut self.current.property_overrides, id, value);
    }

    pub fn restore_property_default(&mut self, id: &str) {
        self.current.property_overrides.remove(id);
    }

    pub fn cancel(&mut self) {
        self.current = self.committed.clone();
        self.current_enabled_displays = self.committed_enabled_displays.clone();
    }

    #[must_use]
    pub fn apply(&mut self) -> WallpaperConfig {
        self.committed = self.current.clone();
        self.committed_enabled_displays = self.current_enabled_displays.clone();
        self.committed.clone()
    }

    fn ensure_monitor_render(&mut self, selector: SerializedSelector) -> &mut MonitorRender {
        if let Some(index) = self
            .current
            .monitors
            .iter()
            .position(|render| render.selector == selector)
        {
            return &mut self.current.monitors[index];
        }

        self.current.monitors.push(MonitorRender {
            selector,
            ..MonitorRender::default()
        });
        self.current
            .monitors
            .last_mut()
            .expect("monitor render was just inserted")
    }
}

fn same_selector_set(left: &[SerializedSelector], right: &[SerializedSelector]) -> bool {
    left.len() == right.len() && left.iter().all(|selector| right.contains(selector))
}
