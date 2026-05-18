use wallpaper_core::{
    DisplayDesc, DisplayIdentity, DisplaySelector, DisplaySnapshotEntry, WallpaperAssignment,
};

use crate::config::{AppConfig, MonitorCfg, SerializedSelector};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorRow {
    pub selector: SerializedSelector,
    pub config: MonitorCfg,
    pub connected: bool,
    pub display_index: Option<usize>,
}

impl AppConfig {
    #[must_use]
    pub fn normalized(&self, displays: &[DisplaySnapshotEntry]) -> Self {
        let mut next = self.clone();
        next.sync_known_monitors(displays);
        next
    }

    #[must_use]
    pub fn monitor_rows(&self, displays: &[DisplaySnapshotEntry]) -> Vec<MonitorRow> {
        let mut rows = Vec::new();
        let primary = displays.first();

        for (index, display) in displays.iter().enumerate() {
            if index > 0 && primary.is_some_and(|primary| display.matches_primary(primary)) {
                continue;
            }

            let selector = if index == 0 {
                SerializedSelector::Primary
            } else {
                display.connected_selector()
            };
            let row_config = self
                .monitors
                .iter()
                .find(|monitor| monitor.selector == selector)
                .cloned()
                .unwrap_or_else(|| MonitorCfg::for_connected_display(selector.clone(), display));

            rows.push(MonitorRow {
                selector,
                config: row_config,
                connected: true,
                display_index: Some(index),
            });
        }

        for monitor in &self.monitors {
            if rows.iter().any(|row| row.selector == monitor.selector) {
                continue;
            }
            if primary.is_some_and(|primary| {
                monitor
                    .selector
                    .to_selector()
                    .matches_primary(primary, displays)
            }) {
                continue;
            }
            rows.push(MonitorRow {
                selector: monitor.selector.clone(),
                config: monitor.clone(),
                connected: false,
                display_index: None,
            });
        }

        rows
    }

    pub fn sync_known_monitors(&mut self, displays: &[DisplaySnapshotEntry]) -> bool {
        let before = self.monitors.clone();
        let mut primary = self
            .monitor_index(&SerializedSelector::Primary)
            .map_or_else(
                || MonitorCfg {
                    selector: SerializedSelector::Primary,
                    ..MonitorCfg::default()
                },
                |index| self.monitors.remove(index),
            );

        primary.selector = SerializedSelector::Primary;
        primary.enabled = true;
        primary.mode = "independent".to_string();
        primary.mirror_target = None;
        self.monitors.insert(0, primary);

        let primary_display = displays.first();
        if let Some(primary) = primary_display {
            let mut carried_wallpaper = None;
            self.monitors.retain(|monitor| {
                if monitor.selector == SerializedSelector::Primary {
                    return true;
                }
                if !matches!(monitor.selector, SerializedSelector::LiveDisplayId { .. }) {
                    return true;
                }

                if monitor
                    .selector
                    .to_selector()
                    .matches_primary(primary, displays)
                {
                    if carried_wallpaper.is_none() {
                        carried_wallpaper.clone_from(&monitor.wallpaper);
                    }
                    false
                } else {
                    true
                }
            });

            if let Some(wallpaper) = carried_wallpaper
                && let Some(primary_monitor) = self
                    .monitors
                    .iter_mut()
                    .find(|monitor| monitor.selector == SerializedSelector::Primary)
                && primary_monitor.wallpaper.is_none()
            {
                primary_monitor.wallpaper = Some(wallpaper);
            }

            if let Some(identity_selector) = primary.stable_identity_selector()
                && let Some(primary_index) = self.monitor_index(&SerializedSelector::Primary)
                && self.monitors[primary_index].wallpaper.is_none()
                && let Some(identity_index) = self.monitor_index(&identity_selector)
            {
                let identity_wallpaper = self.monitors[identity_index].wallpaper.clone();
                self.monitors[primary_index]
                    .wallpaper
                    .clone_from(&identity_wallpaper);
            }
        }

        for (index, display) in displays.iter().enumerate() {
            if index == 0 {
                continue;
            }
            if primary_display.is_some_and(|primary| display.matches_primary(primary)) {
                continue;
            }

            let selector = display.connected_selector();
            if self.monitor_index(&selector).is_none() {
                self.monitors
                    .push(MonitorCfg::for_connected_display(selector, display));
            }
        }

        self.monitors != before
    }

    pub fn ensure_monitor(&mut self, selector: SerializedSelector) -> &mut MonitorCfg {
        if let Some(index) = self.monitor_index(&selector) {
            return &mut self.monitors[index];
        }

        self.monitors.push(MonitorCfg {
            selector,
            ..MonitorCfg::default()
        });
        let index = self.monitors.len() - 1;
        &mut self.monitors[index]
    }

    fn monitor_index(&self, selector: &SerializedSelector) -> Option<usize> {
        self.monitors
            .iter()
            .position(|monitor| &monitor.selector == selector)
    }
}

impl SerializedSelector {
    #[must_use]
    pub fn mirror_target_id(
        &self,
        entry: &DisplaySnapshotEntry,
        displays: &[DisplaySnapshotEntry],
    ) -> Option<String> {
        match self {
            Self::Primary => displays
                .first()
                .filter(|display| display.desc.display_id != entry.desc.display_id)
                .map(|_| self.id()),
            Self::Identity { .. } | Self::LiveDisplayId { .. } => displays
                .iter()
                .find(|display| {
                    display.desc.display_id != entry.desc.display_id
                        && self.to_selector().matches_display(display)
                })
                .map(|display| display.config_selector(displays).id()),
        }
    }
}

impl MonitorCfg {
    #[must_use]
    fn for_connected_display(selector: SerializedSelector, display: &DisplaySnapshotEntry) -> Self {
        let mut monitor = Self {
            selector,
            enabled: display.window_active,
            ..Self::default()
        };
        if let Some(WallpaperAssignment::Mirror(target)) = display.assignment.as_ref() {
            monitor.mode = "mirror".to_string();
            monitor.mirror_target = Some(SerializedSelector::from_selector(target));
        }
        monitor
    }
}

pub trait DisplaySnapshotExt {
    fn connected_selector(&self) -> SerializedSelector;
    fn config_selector(&self, displays: &[DisplaySnapshotEntry]) -> SerializedSelector;
    fn stable_identity_selector(&self) -> Option<SerializedSelector>;
    fn matches_primary(&self, primary: &Self) -> bool;
}

impl DisplaySnapshotExt for DisplaySnapshotEntry {
    fn connected_selector(&self) -> SerializedSelector {
        self.stable_identity_selector()
            .unwrap_or(SerializedSelector::LiveDisplayId {
                display_id: self.desc.display_id,
            })
    }

    fn config_selector(&self, displays: &[DisplaySnapshotEntry]) -> SerializedSelector {
        if displays
            .first()
            .is_some_and(|primary| self.matches_primary(primary))
        {
            SerializedSelector::Primary
        } else {
            self.connected_selector()
        }
    }

    fn stable_identity_selector(&self) -> Option<SerializedSelector> {
        self.identity.has_stable_identity().then(|| {
            SerializedSelector::from_selector(&DisplaySelector::Identity(self.identity.clone()))
        })
    }

    fn matches_primary(&self, primary: &Self) -> bool {
        self.desc.display_id == primary.desc.display_id
            || primary.identity.same_physical_identity(&self.identity)
    }
}

pub trait DisplayIdentityExt {
    fn has_stable_identity(&self) -> bool;
    fn same_physical_identity(&self, other: &Self) -> bool;
}

impl DisplayIdentityExt for DisplayIdentity {
    fn has_stable_identity(&self) -> bool {
        self.uuid.as_deref().is_some_and(|uuid| !uuid.is_empty())
            || (self.vendor_id.is_some()
                && self.model_id.is_some()
                && (self.serial_number.is_some() || self.unit_number.is_some()))
    }

    fn same_physical_identity(&self, other: &Self) -> bool {
        if let (Some(left_uuid), Some(right_uuid)) = (self.uuid.as_deref(), other.uuid.as_deref()) {
            return !left_uuid.is_empty() && left_uuid == right_uuid;
        }

        if self.vendor_id.is_some()
            && self.model_id.is_some()
            && self.serial_number.is_some()
            && self.vendor_id == other.vendor_id
            && self.model_id == other.model_id
            && self.serial_number == other.serial_number
        {
            return true;
        }

        if self.vendor_id.is_some()
            && self.model_id.is_some()
            && self.unit_number.is_some()
            && self.vendor_id == other.vendor_id
            && self.model_id == other.model_id
            && self.unit_number == other.unit_number
        {
            return true;
        }

        false
    }
}

pub trait DisplayDescExt {
    fn same_physical_display(&self, other: &Self) -> bool;
}

impl DisplayDescExt for DisplayDesc {
    fn same_physical_display(&self, other: &Self) -> bool {
        self.display_id == other.display_id || self.identity.same_physical_identity(&other.identity)
    }
}

pub trait DisplaySelectorExt {
    fn matches_primary(
        &self,
        primary: &DisplaySnapshotEntry,
        displays: &[DisplaySnapshotEntry],
    ) -> bool;
    fn matches_display(&self, display: &DisplaySnapshotEntry) -> bool;
    fn resolve_display<'a>(
        &self,
        displays: &'a [DisplaySnapshotEntry],
    ) -> Option<&'a DisplaySnapshotEntry>;
}

impl DisplaySelectorExt for DisplaySelector {
    fn matches_primary(
        &self,
        primary: &DisplaySnapshotEntry,
        displays: &[DisplaySnapshotEntry],
    ) -> bool {
        match self {
            Self::Primary => true,
            Self::LiveDisplayId(display_id) => {
                *display_id == primary.desc.display_id
                    || displays
                        .iter()
                        .find(|display| display.desc.display_id == *display_id)
                        .is_some_and(|display| display.matches_primary(primary))
            }
            Self::Identity(identity) => primary.identity.same_physical_identity(identity),
        }
    }

    fn matches_display(&self, display: &DisplaySnapshotEntry) -> bool {
        match self {
            Self::Primary => false,
            Self::Identity(identity) => display.identity.match_score(identity).is_some(),
            Self::LiveDisplayId(display_id) => display.desc.display_id == *display_id,
        }
    }

    fn resolve_display<'a>(
        &self,
        displays: &'a [DisplaySnapshotEntry],
    ) -> Option<&'a DisplaySnapshotEntry> {
        match self {
            Self::Primary => displays.first(),
            Self::Identity { .. } | Self::LiveDisplayId { .. } => displays
                .iter()
                .find(|display| self.matches_display(display)),
        }
    }
}
