#[cfg(test)]
use crate::DisplayConfig;
use crate::{
    DisplayDesc, DisplayIdentity, DisplaySelector, EngineError, WallpaperAssignment,
    WallpaperEngineConfig,
    project::{SceneDesc, SceneDescSliceExt, SceneTemplate},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DisplayKey {
    Primary,
    Identity(DisplayIdentity),
    LiveDisplayId(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayAction {
    Open(DisplayKey),
    Close(DisplayKey),
    Rebuild(DisplayKey),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayRecord {
    pub key: DisplayKey,
    pub live_display: Option<DisplayDesc>,
    pub assignment: Option<WallpaperAssignment>,
    pub window_active: bool,
    pub runtime_open: bool,
    pub primary_inheritance_consumed: bool,
}

impl DisplayRecord {
    pub fn should_have_runtime(&self) -> bool {
        self.window_active && self.live_display.is_some() && self.assignment.is_some()
    }

    pub fn scene_desc(&self) -> Result<Option<SceneDesc>, EngineError> {
        let Some(display) = self.live_display.clone() else {
            return Ok(None);
        };
        match self.assignment.as_ref() {
            Some(WallpaperAssignment::Direct(template)) => {
                template.validate()?;
                Ok(Some(template.for_display(display)))
            }
            Some(WallpaperAssignment::Mirror(_)) => Err(EngineError::Platform(
                "mirror assignments must be resolved before runtime creation".to_string(),
            )),
            None => Ok(None),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayDebugEvent {
    Connected {
        key: DisplayKey,
        display: DisplayDesc,
    },
    Disconnected {
        key: DisplayKey,
        display: DisplayDesc,
    },
    Changed {
        key: DisplayKey,
        before: DisplayDesc,
        after: DisplayDesc,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayStateModel {
    pub records: Vec<DisplayRecord>,
}

impl DisplayKey {
    /// Returns a debug-friendly label describing this key variant.
    pub fn key_label(&self) -> String {
        match self {
            DisplayKey::Primary => "primary".to_string(),
            DisplayKey::Identity(identity) => format!("identity({})", identity.identity_label()),
            DisplayKey::LiveDisplayId(display_id) => format!("live-display-id({display_id})"),
        }
    }

    pub fn from_selector(selector: &DisplaySelector) -> Result<Self, EngineError> {
        match selector {
            DisplaySelector::Primary => Ok(Self::Primary),
            DisplaySelector::Identity(identity) if !identity.is_empty() => {
                Ok(Self::Identity(identity.clone()))
            }
            DisplaySelector::Identity(_) => Err(EngineError::InvalidInput(
                "display identity selector must not be empty".to_string(),
            )),
            DisplaySelector::LiveDisplayId(0) => Err(EngineError::InvalidInput(
                "display_id must be non-zero".to_string(),
            )),
            DisplaySelector::LiveDisplayId(display_id) => Ok(Self::LiveDisplayId(*display_id)),
        }
    }
}

impl DisplayStateModel {
    pub fn from_config(config: WallpaperEngineConfig) -> Result<Self, EngineError> {
        let mut model = Self {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: None,
                assignment: None,
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            }],
        };

        for display in config.displays {
            if let Some(WallpaperAssignment::Direct(template)) = display.wallpaper.as_ref() {
                template.validate()?;
            }
            let key = DisplayKey::from_selector(&display.selector)?;
            let record = model.ensure_record(key);
            record.window_active = display.window_active;
            record.assignment = display.wallpaper;
        }

        model.reject_mirror_cycles()?;
        Ok(model)
    }

    fn ensure_record(&mut self, key: DisplayKey) -> &mut DisplayRecord {
        if let Some(index) = self.records.iter().position(|record| record.key == key) {
            return &mut self.records[index];
        }
        self.records.push(DisplayRecord {
            key,
            live_display: None,
            assignment: None,
            window_active: false,
            runtime_open: false,
            primary_inheritance_consumed: false,
        });
        self.records.last_mut().expect("record was just pushed")
    }

    pub fn set_assignment(
        &mut self,
        selector: &DisplaySelector,
        assignment: WallpaperAssignment,
    ) -> Result<(), EngineError> {
        if let WallpaperAssignment::Direct(template) = &assignment {
            template.validate()?;
        }
        let key = DisplayKey::from_selector(selector)?;
        let mut candidate = self.clone();
        candidate.ensure_record(key).assignment = Some(assignment);
        candidate.reject_mirror_cycles()?;
        *self = candidate;
        Ok(())
    }

    pub fn refresh_connected(
        &mut self,
        primary: DisplayDesc,
        displays: Vec<DisplayDesc>,
    ) -> Result<Vec<DisplayAction>, EngineError> {
        let mut candidate = self.clone();
        let primary_id = primary.display_id;
        let primary_assignment_before = candidate
            .record(&DisplayKey::Primary)
            .and_then(|record| record.assignment.clone());
        let connected = candidate.connected_display_keys(primary_id, primary, displays)?;

        for record in &mut candidate.records {
            record.live_display = None;
        }

        for (key, display) in connected {
            let record = candidate.ensure_record(key);
            record.live_display = Some(display);
            if record.live_display.as_ref().unwrap().display_id != primary_id
                && record.assignment.is_none()
                && !record.primary_inheritance_consumed
            {
                record.assignment.clone_from(&primary_assignment_before);
                record.primary_inheritance_consumed = true;
            }
        }

        candidate.reject_mirror_cycles()?;
        candidate.transfer_runtime_open_flags_from(self);
        let actions = candidate.plan_actions()?;
        candidate.log_debug_transition(self);
        *self = candidate;
        Ok(actions)
    }

    pub fn set_window_active(
        &mut self,
        selector: &DisplaySelector,
        active: bool,
    ) -> Result<Vec<DisplayAction>, EngineError> {
        let key = DisplayKey::from_selector(selector)?;
        self.ensure_record(key).window_active = active;
        self.plan_actions()
    }

    pub fn resolved_assignment(
        &self,
        key: &DisplayKey,
    ) -> Result<Option<WallpaperAssignment>, EngineError> {
        self.resolve_assignment_inner(key, &mut Vec::new())
    }

    fn resolve_assignment_inner(
        &self,
        key: &DisplayKey,
        stack: &mut Vec<DisplayKey>,
    ) -> Result<Option<WallpaperAssignment>, EngineError> {
        if stack.contains(key) {
            return Err(EngineError::InvalidInput(
                "wallpaper mirror cycle detected".to_string(),
            ));
        }
        stack.push(key.clone());
        let result = match self
            .record(key)
            .and_then(|record| record.assignment.as_ref())
        {
            Some(WallpaperAssignment::Direct(template)) => {
                Some(WallpaperAssignment::Direct(template.clone()))
            }
            Some(WallpaperAssignment::Mirror(selector)) => {
                let source = DisplayKey::from_selector(selector)?;
                self.resolve_assignment_inner(&source, stack)?
            }
            None => None,
        };
        stack.pop();
        Ok(result)
    }

    fn reject_mirror_cycles(&self) -> Result<(), EngineError> {
        for record in &self.records {
            self.resolved_assignment(&record.key)?;
        }
        Ok(())
    }

    fn plan_actions(&self) -> Result<Vec<DisplayAction>, EngineError> {
        let mut actions = Vec::new();
        for record in &self.records {
            let has_assignment = self.resolved_assignment(&record.key)?.is_some();
            let should_be_open =
                record.window_active && record.live_display.is_some() && has_assignment;
            match (record.runtime_open, should_be_open) {
                (false, true) => actions.push(DisplayAction::Open(record.key.clone())),
                (true, false) => actions.push(DisplayAction::Close(record.key.clone())),
                (true, true) => actions.push(DisplayAction::Rebuild(record.key.clone())),
                (false, false) => {}
            }
        }
        Ok(actions)
    }

    fn connected_display_keys(
        &self,
        primary_id: u32,
        primary: DisplayDesc,
        displays: Vec<DisplayDesc>,
    ) -> Result<Vec<(DisplayKey, DisplayDesc)>, EngineError> {
        let mut connected = Vec::with_capacity(displays.len() + 1);
        let primary_key = self.key_for_connected_display(primary_id, &primary)?;
        let primary_for_dedupe = primary.clone();
        connected.push((primary_key, primary));

        for display in displays {
            let primary_identity = &primary_for_dedupe.identity;
            let display_identity = &display.identity;
            let same_physical_identity = !primary_identity.is_empty()
                && !display_identity.is_empty()
                && if let (Some(left_uuid), Some(right_uuid)) = (
                    primary_identity.uuid.as_deref(),
                    display_identity.uuid.as_deref(),
                ) {
                    !left_uuid.is_empty() && left_uuid == right_uuid
                } else if primary_identity.vendor_id.is_some()
                    && primary_identity.model_id.is_some()
                    && primary_identity.serial_number.is_some()
                    && primary_identity.vendor_id == display_identity.vendor_id
                    && primary_identity.model_id == display_identity.model_id
                    && primary_identity.serial_number == display_identity.serial_number
                {
                    true
                } else {
                    primary_identity.vendor_id.is_some()
                        && primary_identity.model_id.is_some()
                        && primary_identity.unit_number.is_some()
                        && primary_identity.vendor_id == display_identity.vendor_id
                        && primary_identity.model_id == display_identity.model_id
                        && primary_identity.unit_number == display_identity.unit_number
                };

            if display.display_id == primary_id || same_physical_identity {
                continue;
            }
            let key = self.key_for_connected_display(primary_id, &display)?;
            connected.push((key, display));
        }

        let mut keys = std::collections::HashSet::with_capacity(connected.len());
        if connected.iter().any(|(key, _)| !keys.insert(key.clone())) {
            return Err(EngineError::InvalidInput(
                "multiple connected displays matched the same display configuration".to_string(),
            ));
        }

        Ok(connected)
    }

    fn transfer_runtime_open_flags_from(&mut self, before: &DisplayStateModel) {
        let mut consumed = vec![false; before.records.len()];

        for record in &mut self.records {
            if !record.should_have_runtime() {
                record.runtime_open = false;
                continue;
            }
            let display = record
                .live_display
                .as_ref()
                .expect("runtime target records have live displays");
            let source_index = before
                .records
                .iter()
                .enumerate()
                .position(|(index, source)| {
                    !consumed[index]
                        && source.runtime_open
                        && source.live_display.as_ref().is_some_and(|source_display| {
                            source_display.is_same_physical_display_as(display)
                        })
                })
                .or_else(|| {
                    before
                        .records
                        .iter()
                        .enumerate()
                        .position(|(index, source)| {
                            !consumed[index] && source.runtime_open && source.key == record.key
                        })
                });

            let Some(source_index) = source_index else {
                record.runtime_open = false;
                continue;
            };

            record.runtime_open = true;
            consumed[source_index] = true;
        }
    }

    fn key_for_connected_display(
        &self,
        primary_id: u32,
        display: &DisplayDesc,
    ) -> Result<DisplayKey, EngineError> {
        if display.display_id == primary_id {
            return Ok(self.primary_or_active_live_display_key(display.display_id));
        }

        if let Some(key) = self.active_live_display_id_key(display.display_id) {
            return Ok(key);
        }

        if !display.identity.is_empty()
            && let Some(key) = self.matching_identity_key(&display.identity)?
        {
            return Ok(key);
        }

        Ok(self
            .assigned_live_display_id_key(display.display_id)
            .unwrap_or_else(|| {
                if display.identity.is_empty() {
                    DisplayKey::LiveDisplayId(display.display_id)
                } else {
                    DisplayKey::Identity(display.identity.clone())
                }
            }))
    }

    fn primary_or_active_live_display_key(&self, display_id: u32) -> DisplayKey {
        let primary_is_configured = self
            .record(&DisplayKey::Primary)
            .is_some_and(|record| record.window_active || record.runtime_open);
        if primary_is_configured {
            return DisplayKey::Primary;
        }
        self.active_live_display_id_key(display_id)
            .unwrap_or(DisplayKey::Primary)
    }

    fn active_live_display_id_key(&self, display_id: u32) -> Option<DisplayKey> {
        self.records.iter().find_map(|record| match record.key {
            DisplayKey::LiveDisplayId(record_display_id)
                if record_display_id == display_id
                    && (record.window_active || record.runtime_open) =>
            {
                Some(record.key.clone())
            }
            _ => None,
        })
    }

    fn assigned_live_display_id_key(&self, display_id: u32) -> Option<DisplayKey> {
        self.records.iter().find_map(|record| match record.key {
            DisplayKey::LiveDisplayId(record_display_id)
                if record_display_id == display_id && record.assignment.is_some() =>
            {
                Some(record.key.clone())
            }
            _ => None,
        })
    }

    fn matching_identity_key(
        &self,
        live_identity: &DisplayIdentity,
    ) -> Result<Option<DisplayKey>, EngineError> {
        let mut matches = Vec::new();
        for record in &self.records {
            let DisplayKey::Identity(existing_identity) = &record.key else {
                continue;
            };
            let Some(score) = existing_identity.match_score(live_identity) else {
                continue;
            };
            matches.push((score, record.key.clone()));
        }

        let Some(max_score) = matches.iter().map(|(score, _)| *score).max() else {
            return Ok(None);
        };
        let mut best_matches = matches
            .into_iter()
            .filter(|(score, _)| *score == max_score)
            .map(|(_, key)| key);
        let best = best_matches
            .next()
            .expect("max score implies at least one match");
        if best_matches.next().is_some() {
            return Err(EngineError::InvalidInput(
                "ambiguous display identity match".to_string(),
            ));
        }

        Ok(Some(best))
    }

    fn record(&self, key: &DisplayKey) -> Option<&DisplayRecord> {
        self.records.iter().find(|record| &record.key == key)
    }

    pub fn apply_reconcile(&mut self, scenes: &[SceneDesc]) -> Result<(), EngineError> {
        for scene in scenes {
            scene.validate()?;
        }
        scenes.assert_unique()?;

        let requested = scenes
            .iter()
            .map(|scene| self.reconcile_key(scene))
            .collect::<Result<std::collections::HashSet<_>, _>>()?;

        for record in &mut self.records {
            if record.live_display.is_some() && !requested.contains(&record.key) {
                record.window_active = false;
            }
        }

        for scene in scenes {
            let key = self.reconcile_key(scene)?;
            let record =
                if let Some(index) = self.records.iter().position(|record| record.key == key) {
                    &mut self.records[index]
                } else {
                    self.records.push(DisplayRecord {
                        key,
                        live_display: None,
                        assignment: None,
                        window_active: false,
                        runtime_open: false,
                        primary_inheritance_consumed: false,
                    });
                    self.records.last_mut().expect("record was just pushed")
                };
            record.live_display = Some(scene.display.clone());
            record.window_active = true;
            record.assignment = Some(WallpaperAssignment::Direct(SceneTemplate::from_scene_desc(
                scene,
            )));
        }

        Ok(())
    }

    pub fn reconcile_key(&self, scene: &SceneDesc) -> Result<DisplayKey, EngineError> {
        if self.records.iter().any(|record| {
            record.key == DisplayKey::Primary
                && record
                    .live_display
                    .as_ref()
                    .is_some_and(|display| display.display_id == scene.display.display_id)
        }) {
            return Ok(DisplayKey::Primary);
        }

        if let Some(key) = self.records.iter().find_map(|record| {
            record.live_display.as_ref().and_then(|display| {
                (display.display_id == scene.display.display_id).then(|| record.key.clone())
            })
        }) {
            return Ok(key);
        }

        if !scene.display.identity.is_empty()
            && let Some(key) = self.matching_identity_key(&scene.display.identity)?
        {
            return Ok(key);
        }

        Ok(DisplayKey::LiveDisplayId(scene.display.display_id))
    }

    pub fn has_live_id(&self, display_id: u32) -> bool {
        self.records.iter().any(|record| {
            record
                .live_display
                .as_ref()
                .is_some_and(|display| display.display_id == display_id)
        })
    }

    pub fn debug_events_since(&self, before: &DisplayStateModel) -> Vec<DisplayDebugEvent> {
        let mut events = Vec::new();

        for record in self
            .records
            .iter()
            .filter(|record| record.live_display.is_some())
        {
            let display = record
                .live_display
                .as_ref()
                .expect("filtered to records with live displays");
            if !before.has_live_id(display.display_id) {
                events.push(DisplayDebugEvent::Connected {
                    key: record.key.clone(),
                    display: display.clone(),
                });
            }
        }

        for record in before
            .records
            .iter()
            .filter(|record| record.live_display.is_some())
        {
            let display = record
                .live_display
                .as_ref()
                .expect("filtered to records with live displays");
            if !self.has_live_id(display.display_id) {
                events.push(DisplayDebugEvent::Disconnected {
                    key: record.key.clone(),
                    display: display.clone(),
                });
            }
        }

        for after_record in self
            .records
            .iter()
            .filter(|record| record.live_display.is_some())
        {
            let Some(before_display) = before
                .record(&after_record.key)
                .and_then(|record| record.live_display.as_ref())
            else {
                continue;
            };
            let after_display = after_record
                .live_display
                .as_ref()
                .expect("filtered to records with live displays");
            if !before_display.has_same_geometry(after_display) {
                events.push(DisplayDebugEvent::Changed {
                    key: after_record.key.clone(),
                    before: before_display.clone(),
                    after: after_display.clone(),
                });
            }
        }

        events
    }

    pub fn log_debug_transition(&self, before: &DisplayStateModel) {
        for event in self.debug_events_since(before) {
            eprintln!("{}", event.debug_message());
        }
    }
}

impl DisplayDebugEvent {
    fn debug_message(&self) -> String {
        match self {
            Self::Connected { key, display } => format!(
                "[wallpaper-core display] monitor connected: key={} {}",
                key.key_label(),
                display.desc_label()
            ),
            Self::Disconnected { key, display } => format!(
                "[wallpaper-core display] monitor disconnected: key={} {}",
                key.key_label(),
                display.desc_label()
            ),
            Self::Changed { key, before, after } => format!(
                "[wallpaper-core display] display geometry changed: key={} before=[{}] after=[{}]",
                key.key_label(),
                before.desc_label(),
                after.desc_label()
            ),
        }
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u32, identity: DisplayIdentity) -> DisplayDesc {
        DisplayDesc::with_identity(id, identity, 0, 0, 1920, 1080, 1.0)
    }

    fn scaled_display(
        id: u32,
        identity: DisplayIdentity,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> DisplayDesc {
        DisplayDesc::with_identity(id, identity, 0, 0, width, height, scale_factor)
    }

    fn identity(label: &str) -> DisplayIdentity {
        DisplayIdentity {
            uuid: Some(label.to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(u32::from(label.bytes().next().unwrap_or_default())),
            unit_number: Some(1),
            name: None,
        }
    }

    fn vendor_model_unit(vendor_id: u32, model_id: u32, unit_number: u32) -> DisplayIdentity {
        DisplayIdentity {
            uuid: None,
            vendor_id: Some(vendor_id),
            model_id: Some(model_id),
            serial_number: None,
            unit_number: Some(unit_number),
            name: None,
        }
    }

    fn template(path: &str) -> SceneTemplate {
        SceneTemplate::builder(path)
            .build()
            .expect("template should build")
    }

    #[test]
    fn default_config_creates_active_blank_primary_record() {
        let model = DisplayStateModel::from_config(WallpaperEngineConfig::default()).unwrap();

        assert_eq!(model.records.len(), 1);
        assert_eq!(model.records[0].key, DisplayKey::Primary);
        assert!(model.records[0].window_active);
        assert!(model.records[0].assignment.is_none());
        assert!(!model.records[0].runtime_open);
    }

    #[test]
    fn future_display_config_is_stored_without_live_display() {
        let external = identity("external");
        let model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(external.clone()),
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/project.json"))),
            }],
        })
        .unwrap();

        assert!(model.records.iter().any(|record| {
            record.key == DisplayKey::Identity(external.clone())
                && record.window_active
                && record.live_display.is_none()
                && record.assignment.is_some()
        }));
    }

    #[test]
    fn connecting_secondary_inherits_primary_assignment_once_but_stays_inactive() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig::default()).unwrap();
        model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Direct(template("/tmp/primary.json")),
            )
            .unwrap();

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();

        let secondary = model
            .records
            .iter()
            .find(|record| record.key == DisplayKey::Identity(external.clone()))
            .expect("secondary record should exist");
        assert_eq!(
            secondary.assignment,
            Some(WallpaperAssignment::Direct(template("/tmp/primary.json")))
        );
        assert!(!secondary.window_active);

        model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Direct(template("/tmp/new-primary.json")),
            )
            .unwrap();
        let secondary = model
            .records
            .iter()
            .find(|record| matches!(record.key, DisplayKey::Identity(_)))
            .expect("secondary record should remain");
        assert_eq!(
            secondary.assignment,
            Some(WallpaperAssignment::Direct(template("/tmp/primary.json")))
        );
    }

    #[test]
    fn refresh_preserves_explicit_live_display_id_record() {
        let external = identity("external");
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: None,
                    assignment: None,
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::LiveDisplayId(2),
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/live.json"))),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();

        let live_record = model
            .record(&DisplayKey::LiveDisplayId(2))
            .expect("legacy live-id record should remain");
        assert_eq!(live_record.live_display, Some(display(2, external)));
        assert!(
            !model
                .records
                .iter()
                .any(|record| matches!(record.key, DisplayKey::Identity(_)))
        );
    }

    #[test]
    fn refresh_skips_primary_duplicate_with_different_live_id() {
        let primary = identity("primary");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig::default()).unwrap();

        model
            .refresh_connected(
                display(1, primary.clone()),
                vec![display(1, primary.clone()), display(99, primary.clone())],
            )
            .unwrap();

        assert_eq!(model.records.len(), 1);
        let primary_record = model
            .record(&DisplayKey::Primary)
            .expect("primary record should remain");
        assert_eq!(
            primary_record.live_display,
            Some(display(1, primary.clone()))
        );
        assert!(
            !model
                .records
                .iter()
                .any(|record| record.key == DisplayKey::Identity(primary.clone())
                    || record.key == DisplayKey::LiveDisplayId(99))
        );
    }

    #[test]
    fn reconcile_scene_for_connected_identity_display_uses_identity_record() {
        let primary = identity("primary");
        let external = identity("external");
        let external_display = display(2, external.clone());
        let scene = SceneDesc::new(
            external_display.clone(),
            "/tmp/requested.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(display(1, primary)),
                    assignment: None,
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(external.clone()),
                    live_display: Some(external_display.clone()),
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/old.json"))),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model.apply_reconcile(&[scene]).unwrap();

        let record = model
            .record(&DisplayKey::Identity(external))
            .expect("identity record should remain");
        assert!(record.window_active);
        assert_eq!(record.live_display, Some(external_display));
        assert!(matches!(
            record.assignment,
            Some(WallpaperAssignment::Direct(ref template))
                if template.scene_path == "/tmp/requested.json"
        ));
        assert!(
            !model
                .records
                .iter()
                .any(|record| record.key == DisplayKey::LiveDisplayId(2))
        );
    }

    #[test]
    fn display_debug_events_report_connected_and_disconnected_displays() {
        let primary = identity("primary");
        let external = identity("external");
        let before = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(display(1, primary.clone())),
                    assignment: None,
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(external.clone()),
                    live_display: None,
                    assignment: None,
                    window_active: false,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };
        let after_connect = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(display(1, primary.clone())),
                    assignment: None,
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(external.clone()),
                    live_display: Some(display(2, external.clone())),
                    assignment: None,
                    window_active: false,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        assert_eq!(
            after_connect.debug_events_since(&before),
            vec![DisplayDebugEvent::Connected {
                key: DisplayKey::Identity(external.clone()),
                display: display(2, external.clone()),
            }]
        );

        assert_eq!(
            before.debug_events_since(&after_connect),
            vec![DisplayDebugEvent::Disconnected {
                key: DisplayKey::Identity(external.clone()),
                display: display(2, external),
            }]
        );
    }

    #[test]
    fn display_debug_events_report_logical_resolution_and_scale_changes() {
        let primary = identity("primary");
        let before = DisplayStateModel {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(scaled_display(1, primary.clone(), 3840, 2160, 2.0)),
                assignment: None,
                window_active: true,
                runtime_open: true,
                primary_inheritance_consumed: false,
            }],
        };
        let after = DisplayStateModel {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(scaled_display(1, primary.clone(), 3840, 2160, 1.5)),
                assignment: None,
                window_active: true,
                runtime_open: true,
                primary_inheritance_consumed: false,
            }],
        };

        assert_eq!(
            after.debug_events_since(&before),
            vec![DisplayDebugEvent::Changed {
                key: DisplayKey::Primary,
                before: scaled_display(1, primary.clone(), 3840, 2160, 2.0),
                after: scaled_display(1, primary, 3840, 2160, 1.5),
            }]
        );
    }

    #[test]
    fn display_debug_events_report_primary_replacement_as_descriptor_change() {
        let before = DisplayStateModel {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(scaled_display(1, identity("old"), 3420, 2214, 2.0)),
                assignment: None,
                window_active: true,
                runtime_open: true,
                primary_inheritance_consumed: false,
            }],
        };
        let after = DisplayStateModel {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(scaled_display(2, identity("new"), 1920, 1080, 1.0)),
                assignment: None,
                window_active: true,
                runtime_open: true,
                primary_inheritance_consumed: false,
            }],
        };

        assert_eq!(
            after.debug_events_since(&before),
            vec![
                DisplayDebugEvent::Connected {
                    key: DisplayKey::Primary,
                    display: scaled_display(2, identity("new"), 1920, 1080, 1.0),
                },
                DisplayDebugEvent::Disconnected {
                    key: DisplayKey::Primary,
                    display: scaled_display(1, identity("old"), 3420, 2214, 2.0),
                },
                DisplayDebugEvent::Changed {
                    key: DisplayKey::Primary,
                    before: scaled_display(1, identity("old"), 3420, 2214, 2.0),
                    after: scaled_display(2, identity("new"), 1920, 1080, 1.0),
                },
            ]
        );
    }

    #[test]
    fn partial_identity_config_matches_full_live_identity() {
        let partial = DisplayIdentity {
            uuid: Some("external".to_string()),
            ..DisplayIdentity::default()
        };
        let full = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(partial.clone()),
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/external.json"))),
            }],
        })
        .unwrap();

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary")), display(2, full.clone())],
            )
            .unwrap();

        let matched = model
            .record(&DisplayKey::Identity(partial))
            .expect("partial identity record should remain");
        assert_eq!(matched.live_display, Some(display(2, full)));
        assert!(
            !model
                .records
                .iter()
                .any(|record| record.key == DisplayKey::Identity(identity("external")))
        );
    }

    #[test]
    fn ambiguous_identity_match_errors_and_preserves_model() {
        let mut first = vendor_model_unit(7, 8, 9);
        first.name = Some("first".to_string());
        let mut second = vendor_model_unit(7, 8, 9);
        second.name = Some("second".to_string());
        let live = DisplayIdentity {
            uuid: Some("live".to_string()),
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(10),
            unit_number: Some(9),
            name: Some("connected".to_string()),
        };
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![
                DisplayConfig {
                    selector: DisplaySelector::Identity(first),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/first.json"))),
                },
                DisplayConfig {
                    selector: DisplaySelector::Identity(second),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/second.json"))),
                },
            ],
        })
        .unwrap();
        let before = model.clone();

        let error = model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary")), display(2, live)],
            )
            .expect_err("ambiguous identity match should fail");

        match error {
            EngineError::InvalidInput(message) => assert!(message.contains("ambiguous")),
            other => panic!("expected invalid input, got {other:?}"),
        }
        assert_eq!(model, before);
    }

    #[test]
    fn unique_higher_score_identity_match_wins_over_lower_score_ties() {
        let mut lower_first = vendor_model_unit(7, 8, 9);
        lower_first.name = Some("lower-first".to_string());
        let mut lower_second = vendor_model_unit(7, 8, 9);
        lower_second.name = Some("lower-second".to_string());
        let higher = DisplayIdentity {
            uuid: None,
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(10),
            unit_number: None,
            name: Some("higher".to_string()),
        };
        let live = DisplayIdentity {
            uuid: Some("live".to_string()),
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(10),
            unit_number: Some(9),
            name: Some("connected".to_string()),
        };
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![
                DisplayConfig {
                    selector: DisplaySelector::Identity(lower_first),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/lower-a.json"))),
                },
                DisplayConfig {
                    selector: DisplaySelector::Identity(lower_second),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/lower-b.json"))),
                },
                DisplayConfig {
                    selector: DisplaySelector::Identity(higher.clone()),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/higher.json"))),
                },
            ],
        })
        .unwrap();

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary")), display(2, live.clone())],
            )
            .unwrap();

        let matched = model
            .record(&DisplayKey::Identity(higher))
            .expect("higher score identity should remain");
        assert_eq!(matched.live_display, Some(display(2, live)));
    }

    #[test]
    fn primary_refresh_prefers_primary_record_over_stale_live_id() {
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::LiveDisplayId(1),
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/stale.json"))),
                    window_active: false,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary"))],
            )
            .unwrap();

        assert_eq!(
            model.record(&DisplayKey::Primary).unwrap().live_display,
            Some(display(1, identity("primary")))
        );
        assert_eq!(
            model
                .record(&DisplayKey::LiveDisplayId(1))
                .unwrap()
                .live_display,
            None
        );
    }

    #[test]
    fn active_live_id_primary_wins_over_inactive_primary_assignment() {
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
                    window_active: false,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::LiveDisplayId(1),
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/live.json"))),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary"))],
            )
            .unwrap();

        assert_eq!(
            model.record(&DisplayKey::Primary).unwrap().live_display,
            None
        );
        assert_eq!(
            model
                .record(&DisplayKey::LiveDisplayId(1))
                .unwrap()
                .live_display,
            Some(display(1, identity("primary")))
        );
    }

    #[test]
    fn inactive_live_id_assignment_does_not_mask_matching_identity() {
        let external = identity("external");
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: None,
                    assignment: None,
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(external.clone()),
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/identity.json"))),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::LiveDisplayId(2),
                    live_display: None,
                    assignment: Some(WallpaperAssignment::Direct(template("/tmp/stale.json"))),
                    window_active: false,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();

        assert_eq!(
            model
                .record(&DisplayKey::Identity(external.clone()))
                .unwrap()
                .live_display,
            Some(display(2, external))
        );
        assert_eq!(
            model
                .record(&DisplayKey::LiveDisplayId(2))
                .unwrap()
                .live_display,
            None
        );
    }

    #[test]
    fn duplicate_connected_identity_matches_error_and_preserve_model() {
        let configured = DisplayIdentity {
            uuid: None,
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(9),
            unit_number: None,
            name: None,
        };
        let first = DisplayIdentity {
            uuid: None,
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(9),
            unit_number: Some(1),
            name: Some("first".to_string()),
        };
        let second = DisplayIdentity {
            uuid: None,
            vendor_id: Some(7),
            model_id: Some(8),
            serial_number: Some(9),
            unit_number: Some(2),
            name: Some("second".to_string()),
        };
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(configured),
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/external.json"))),
            }],
        })
        .unwrap();
        let before = model.clone();

        let error = model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, first),
                    display(3, second),
                ],
            )
            .expect_err("duplicate connected display match should fail");

        match error {
            EngineError::InvalidInput(message) => {
                assert!(message.contains("multiple connected displays"));
            }
            other => panic!("expected invalid input, got {other:?}"),
        }
        assert_eq!(model, before);
    }

    #[test]
    fn active_future_display_without_wallpaper_inherits_primary_on_first_connection() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![
                DisplayConfig {
                    selector: DisplaySelector::Primary,
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
                },
                DisplayConfig {
                    selector: DisplaySelector::Identity(external.clone()),
                    window_active: true,
                    wallpaper: None,
                },
            ],
        })
        .unwrap();

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();

        let future = model
            .record(&DisplayKey::Identity(external))
            .expect("future display should connect to existing config");
        assert_eq!(
            future.assignment,
            Some(WallpaperAssignment::Direct(template("/tmp/primary.json")))
        );
    }

    #[test]
    fn blank_primary_at_first_future_connection_prevents_later_auto_inherit() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(external.clone()),
                window_active: true,
                wallpaper: None,
            }],
        })
        .unwrap();

        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();
        assert_eq!(
            model
                .record(&DisplayKey::Identity(external.clone()))
                .expect("future display should connect")
                .assignment,
            None
        );

        model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Direct(template("/tmp/later-primary.json")),
            )
            .unwrap();
        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary"))],
            )
            .unwrap();
        model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .unwrap();

        assert_eq!(
            model
                .record(&DisplayKey::Identity(external))
                .expect("future display should reconnect")
                .assignment,
            None
        );
    }

    #[test]
    fn primary_switch_transfers_runtime_flags_between_primary_and_identity_records() {
        let primary_identity = identity("primary-a");
        let secondary_identity = identity("secondary-b");
        let shared = WallpaperAssignment::Direct(template("/tmp/shared.json"));
        let old_primary =
            DisplayDesc::with_identity(1, primary_identity.clone(), 0, 0, 3420, 2214, 2.0);
        let old_secondary =
            DisplayDesc::with_identity(3, secondary_identity.clone(), 3420, 0, 1920, 1080, 1.0);
        let new_primary =
            DisplayDesc::with_identity(3, secondary_identity.clone(), 0, 0, 1920, 1080, 1.0);
        let new_secondary =
            DisplayDesc::with_identity(1, primary_identity.clone(), -1710, 0, 3420, 2214, 2.0);
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(old_primary),
                    assignment: Some(shared.clone()),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(primary_identity.clone()),
                    live_display: None,
                    assignment: Some(shared.clone()),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(secondary_identity.clone()),
                    live_display: Some(old_secondary),
                    assignment: Some(shared),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        let actions = model
            .refresh_connected(
                new_primary.clone(),
                vec![new_primary.clone(), new_secondary.clone()],
            )
            .unwrap();

        let primary = model
            .record(&DisplayKey::Primary)
            .expect("primary record should remain");
        assert_eq!(primary.live_display, Some(new_primary));
        assert!(primary.runtime_open);

        let new_secondary_record = model
            .record(&DisplayKey::Identity(primary_identity))
            .expect("old primary identity record should remain");
        assert_eq!(new_secondary_record.live_display, Some(new_secondary));
        assert!(new_secondary_record.runtime_open);

        let old_secondary_record = model
            .record(&DisplayKey::Identity(secondary_identity))
            .expect("new primary identity record should remain for future swaps");
        assert_eq!(old_secondary_record.live_display, None);
        assert!(!old_secondary_record.runtime_open);
        assert_eq!(
            actions,
            vec![
                DisplayAction::Rebuild(DisplayKey::Primary),
                DisplayAction::Rebuild(DisplayKey::Identity(identity("primary-a"))),
            ]
        );
    }

    #[test]
    fn mirror_assignment_follows_source_assignment() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(external.clone()),
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Mirror(DisplaySelector::Primary)),
            }],
        })
        .unwrap();
        model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Direct(template("/tmp/primary.json")),
            )
            .unwrap();

        assert_eq!(
            model
                .resolved_assignment(&DisplayKey::Identity(external))
                .unwrap(),
            Some(WallpaperAssignment::Direct(template("/tmp/primary.json")))
        );
    }

    #[test]
    fn mirror_cycles_are_rejected() {
        let a = identity("a");
        let b = identity("b");
        let error = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![
                DisplayConfig {
                    selector: DisplaySelector::Identity(a.clone()),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Mirror(DisplaySelector::Identity(
                        b.clone(),
                    ))),
                },
                DisplayConfig {
                    selector: DisplaySelector::Identity(b),
                    window_active: true,
                    wallpaper: Some(WallpaperAssignment::Mirror(DisplaySelector::Identity(a))),
                },
            ],
        })
        .expect_err("cycle should fail");

        match error {
            EngineError::InvalidInput(message) => assert!(message.contains("mirror cycle")),
            other => panic!("expected invalid input, got {other:?}"),
        }
    }

    #[test]
    fn set_assignment_rejects_cycle_without_mutating_model() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Identity(external.clone()),
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/external.json"))),
            }],
        })
        .unwrap();
        let before = model.clone();

        let error = model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Mirror(DisplaySelector::Primary),
            )
            .expect_err("self-cycle should fail");

        match error {
            EngineError::InvalidInput(message) => assert!(message.contains("mirror cycle")),
            other => panic!("expected invalid input, got {other:?}"),
        }
        assert_eq!(model, before);
    }

    #[test]
    fn failed_refresh_from_inherited_mirror_cycle_keeps_previous_state() {
        let external = identity("external");
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig::default()).unwrap();
        model
            .set_assignment(
                &DisplaySelector::Primary,
                WallpaperAssignment::Mirror(DisplaySelector::Identity(external.clone())),
            )
            .unwrap();
        let before = model.clone();

        let error = model
            .refresh_connected(
                display(1, identity("primary")),
                vec![
                    display(1, identity("primary")),
                    display(2, external.clone()),
                ],
            )
            .expect_err("inherited mirror cycle should fail");

        match error {
            EngineError::InvalidInput(message) => assert!(message.contains("mirror cycle")),
            other => panic!("expected invalid input, got {other:?}"),
        }
        assert_eq!(model, before);
    }

    #[test]
    fn active_connected_assigned_display_plans_open() {
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Primary,
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
            }],
        })
        .unwrap();

        let actions = model
            .refresh_connected(
                display(1, identity("primary")),
                vec![display(1, identity("primary"))],
            )
            .unwrap();

        assert_eq!(actions, vec![DisplayAction::Open(DisplayKey::Primary)]);
    }

    #[test]
    fn plan_actions_returns_mirror_resolution_errors() {
        let external = identity("external");
        let model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(display(1, identity("primary"))),
                    assignment: Some(WallpaperAssignment::Mirror(DisplaySelector::Identity(
                        external.clone(),
                    ))),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(external.clone()),
                    live_display: Some(display(2, external.clone())),
                    assignment: Some(WallpaperAssignment::Mirror(DisplaySelector::Primary)),
                    window_active: true,
                    runtime_open: false,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        let error = model
            .plan_actions()
            .expect_err("mirror cycle should fail during planning");

        match error {
            EngineError::InvalidInput(message) => assert!(message.contains("mirror cycle")),
            other => panic!("expected invalid input, got {other:?}"),
        }
    }

    #[test]
    fn active_already_open_assigned_display_plans_rebuild() {
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Primary,
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
            }],
        })
        .unwrap();
        let record = model.ensure_record(DisplayKey::Primary);
        record.live_display = Some(display(1, identity("primary")));
        record.runtime_open = true;

        assert_eq!(
            model.plan_actions().unwrap(),
            vec![DisplayAction::Rebuild(DisplayKey::Primary)]
        );
    }

    #[test]
    fn open_display_losing_assignment_or_live_display_plans_close() {
        let mut missing_assignment =
            DisplayStateModel::from_config(WallpaperEngineConfig::default()).unwrap();
        {
            let record = missing_assignment.ensure_record(DisplayKey::Primary);
            record.live_display = Some(display(1, identity("primary")));
            record.runtime_open = true;
        }

        assert_eq!(
            missing_assignment.plan_actions().unwrap(),
            vec![DisplayAction::Close(DisplayKey::Primary)]
        );

        let mut missing_live_display = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Primary,
                window_active: true,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
            }],
        })
        .unwrap();
        missing_live_display
            .ensure_record(DisplayKey::Primary)
            .runtime_open = true;

        assert_eq!(
            missing_live_display.plan_actions().unwrap(),
            vec![DisplayAction::Close(DisplayKey::Primary)]
        );
    }

    #[test]
    fn inactive_connected_assigned_display_plans_no_action() {
        let mut model = DisplayStateModel::from_config(WallpaperEngineConfig {
            displays: vec![DisplayConfig {
                selector: DisplaySelector::Primary,
                window_active: false,
                wallpaper: Some(WallpaperAssignment::Direct(template("/tmp/primary.json"))),
            }],
        })
        .unwrap();
        model.ensure_record(DisplayKey::Primary).live_display =
            Some(display(1, identity("primary")));

        assert!(model.plan_actions().unwrap().is_empty());
    }
}
