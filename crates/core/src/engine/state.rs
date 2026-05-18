use std::collections::HashMap;

use crate::{
    DisplaySnapshotEntry, EngineError,
    display::state::{DisplayKey, DisplayRecord, DisplayStateModel},
    engine::{
        runtime::{SceneRuntime, SceneRuntimeState},
        snapshot::EngineSnapshot,
    },
    project::SceneHandle,
};

pub struct EngineState {
    /// Display-oriented records that combine persistent model state with
    /// runtime ownership.
    pub display_records: Vec<DisplayRuntimeRecord>,
    /// Display-to-handle index used to preserve handles across reconciliation.
    pub handles_by_display: HashMap<DisplayKey, SceneHandle>,
    /// Handle-to-display index used to route public scene-handle APIs.
    pub displays_by_handle: HashMap<SceneHandle, DisplayKey>,
    /// Next handle candidate. Handles are never reused within one engine.
    pub next_handle: u64,
}

pub struct DisplayRuntimeRecord {
    pub model: DisplayRecord,
    pub handle: Option<SceneHandle>,
    pub runtime: Option<SceneRuntime>,
    pub last_runtime_state: Option<SceneRuntimeState>,
}

impl DisplayRuntimeRecord {
    pub fn should_have_runtime(&self) -> bool {
        self.model.should_have_runtime()
    }

    pub fn scene_desc(&self) -> Result<Option<crate::project::SceneDesc>, EngineError> {
        self.model.scene_desc()
    }
}

impl EngineState {
    pub fn with_display_model(model: DisplayStateModel) -> Self {
        Self {
            display_records: model
                .records
                .into_iter()
                .map(|model| DisplayRuntimeRecord {
                    model,
                    handle: None,
                    runtime: None,
                    last_runtime_state: None,
                })
                .collect(),
            handles_by_display: HashMap::new(),
            displays_by_handle: HashMap::new(),
            next_handle: 1,
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            displays: self
                .display_records
                .iter()
                .filter_map(|record| {
                    let desc = record.model.live_display.clone()?;
                    Some(DisplaySnapshotEntry {
                        identity: desc.identity.clone(),
                        desc,
                        handle: record.handle,
                        window_active: record.model.window_active,
                        assignment: record.model.assignment.clone(),
                    })
                })
                .collect(),
        }
    }

    pub fn record_index(&self, key: &DisplayKey) -> Option<usize> {
        self.display_records
            .iter()
            .position(|record| &record.model.key == key)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ensure_record(&mut self, model: DisplayRecord) -> &mut DisplayRuntimeRecord {
        if let Some(index) = self.record_index(&model.key) {
            self.display_records[index].model = model;
            return &mut self.display_records[index];
        }
        self.display_records.push(DisplayRuntimeRecord {
            model,
            handle: None,
            runtime: None,
            last_runtime_state: None,
        });
        self.display_records
            .last_mut()
            .expect("record was just pushed")
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn apply_display_model(&mut self, model: &DisplayStateModel) -> Result<(), EngineError> {
        let mut previous = std::mem::take(&mut self.display_records);
        let mut next_records = Vec::with_capacity(model.records.len().max(previous.len()));

        for model_record in model.records.iter().cloned() {
            let source_index = if model_record.should_have_runtime() {
                let display = model_record
                    .live_display
                    .as_ref()
                    .expect("runtime target records have live displays");
                previous.iter().position(|record| {
                    record
                        .model
                        .live_display
                        .as_ref()
                        .is_some_and(|source_display| {
                            source_display.is_same_physical_display_as(display)
                        })
                })
            } else {
                None
            }
            .or_else(|| {
                previous
                    .iter()
                    .position(|record| record.model.key == model_record.key)
            });

            if let Some(source_index) = source_index {
                let mut record = previous.swap_remove(source_index);
                record.model = model_record;
                next_records.push(record);
            } else {
                next_records.push(DisplayRuntimeRecord {
                    model: model_record,
                    handle: None,
                    runtime: None,
                    last_runtime_state: None,
                });
            }
        }

        for record in previous {
            if next_records
                .iter()
                .any(|next| next.model.key == record.model.key)
            {
                continue;
            }
            next_records.push(record);
        }
        self.display_records = next_records;
        self.rebuild_handle_indexes();
        Ok(())
    }

    fn rebuild_handle_indexes(&mut self) {
        self.handles_by_display.clear();
        self.displays_by_handle.clear();
        for record in &self.display_records {
            let Some(handle) = record.handle else {
                continue;
            };
            self.handles_by_display
                .insert(record.model.key.clone(), handle);
            self.displays_by_handle
                .insert(handle, record.model.key.clone());
        }
    }

    pub fn reserve_handle_for_key(&mut self, key: DisplayKey) -> SceneHandle {
        if let Some(handle) = self.handles_by_display.get(&key).copied() {
            return handle;
        }
        let handle = SceneHandle::new(self.next_handle);
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        self.handles_by_display.insert(key.clone(), handle);
        self.displays_by_handle.insert(handle, key);
        handle
    }

    pub fn sync_handle_reservation(&mut self, key: DisplayKey, handle: Option<SceneHandle>) {
        if let Some(previous_handle) = self.handles_by_display.remove(&key) {
            self.displays_by_handle.remove(&previous_handle);
        }
        self.displays_by_handle
            .retain(|_, display_key| display_key != &key);

        if let Some(handle) = handle {
            self.handles_by_display.insert(key.clone(), handle);
            self.displays_by_handle.insert(handle, key);
        }
    }

    pub fn scene_mut(&mut self, handle: SceneHandle) -> Result<&mut SceneRuntime, EngineError> {
        let key = self
            .displays_by_handle
            .get(&handle)
            .cloned()
            .ok_or_else(|| {
                EngineError::InvalidInput(format!("unknown scene handle {}", handle.raw()))
            })?;
        let index = self.record_index(&key).ok_or_else(|| {
            EngineError::Platform(format!(
                "scene handle {} pointed at missing display",
                handle.raw()
            ))
        })?;
        self.display_records[index].runtime.as_mut().ok_or_else(|| {
            EngineError::InvalidInput(format!("scene handle {} is not active", handle.raw()))
        })
    }

    pub fn active_runtime_handles(&self) -> impl Iterator<Item = SceneHandle> + '_ {
        self.display_records.iter().filter_map(|record| {
            record.runtime.as_ref()?;
            record.handle
        })
    }

    pub fn record_runtime_state(&mut self, handle: SceneHandle) -> Result<(), EngineError> {
        let key = self
            .displays_by_handle
            .get(&handle)
            .cloned()
            .ok_or_else(|| {
                EngineError::InvalidInput(format!("unknown scene handle {}", handle.raw()))
            })?;
        let index = self.record_index(&key).ok_or_else(|| {
            EngineError::Platform(format!(
                "scene handle {} pointed at missing display",
                handle.raw()
            ))
        })?;
        let runtime_state = self.display_records[index]
            .runtime
            .as_ref()
            .map(SceneRuntime::runtime_state)
            .ok_or_else(|| {
                EngineError::InvalidInput(format!("scene handle {} is not active", handle.raw()))
            })?;
        self.display_records[index].handle = Some(handle);
        self.display_records[index].last_runtime_state = Some(runtime_state);
        self.display_records[index].model.runtime_open = true;
        Ok(())
    }

    pub fn close_all(&mut self) -> Result<(), EngineError> {
        // Detach runtimes from the registry before closing them so callbacks
        // and later actor messages observe a closed state immediately.
        self.handles_by_display.clear();
        self.displays_by_handle.clear();
        let mut runtimes = Vec::new();
        for record in &mut self.display_records {
            record.handle = None;
            record.model.runtime_open = false;
            if let Some(runtime) = record.runtime.as_ref() {
                record.last_runtime_state = Some(runtime.runtime_state());
            }
            if let Some(runtime) = record.runtime.take() {
                runtimes.push(runtime);
            }
        }
        for mut runtime in runtimes {
            runtime.close()?;
        }
        Ok(())
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default())
                .expect("default display config should be valid"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::EngineState;
    use crate::{
        DisplayDesc, DisplayIdentity, WallpaperAssignment,
        display::state::{DisplayKey, DisplayRecord, DisplayStateModel},
        engine::runtime::SceneRuntimeState,
        project::{SceneDesc, SceneTemplate},
    };

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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn display_model_update_transfers_handles_by_physical_display() {
        let primary_identity = identity("primary-a");
        let secondary_identity = identity("secondary-b");
        let shared = WallpaperAssignment::Direct(
            SceneTemplate::builder("/tmp/shared.json")
                .build()
                .expect("template should build"),
        );
        let old_primary =
            DisplayDesc::with_identity(1, primary_identity.clone(), 0, 0, 3420, 2214, 2.0);
        let old_secondary =
            DisplayDesc::with_identity(3, secondary_identity.clone(), 3420, 0, 1920, 1080, 1.0);
        let new_primary =
            DisplayDesc::with_identity(3, secondary_identity.clone(), 0, 0, 1920, 1080, 1.0);
        let new_secondary =
            DisplayDesc::with_identity(1, primary_identity.clone(), -1710, 0, 3420, 2214, 2.0);
        let mut state = EngineState::with_display_model(DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(old_primary.clone()),
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
                    live_display: Some(old_secondary.clone()),
                    assignment: Some(shared.clone()),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        });
        let primary_handle = state.reserve_handle_for_key(DisplayKey::Primary);
        let secondary_key = DisplayKey::Identity(secondary_identity.clone());
        let secondary_handle = state.reserve_handle_for_key(secondary_key.clone());
        {
            let primary = state
                .display_records
                .iter_mut()
                .find(|record| record.model.key == DisplayKey::Primary)
                .expect("primary record should exist");
            primary.handle = Some(primary_handle);
            primary.last_runtime_state = Some(
                SceneRuntimeState::try_from(&SceneDesc::new(
                    old_primary,
                    "/tmp/shared.json",
                    "/tmp/assets",
                    60,
                    false,
                ))
                .unwrap(),
            );
        }
        {
            let secondary = state
                .display_records
                .iter_mut()
                .find(|record| record.model.key == secondary_key)
                .expect("secondary record should exist");
            secondary.handle = Some(secondary_handle);
            secondary.last_runtime_state = Some(
                SceneRuntimeState::try_from(&SceneDesc::new(
                    old_secondary,
                    "/tmp/shared.json",
                    "/tmp/assets",
                    60,
                    false,
                ))
                .unwrap(),
            );
        }

        let mut next = DisplayStateModel {
            records: state
                .display_records
                .iter()
                .map(|record| record.model.clone())
                .collect(),
        };
        next.refresh_connected(
            new_primary.clone(),
            vec![new_primary, new_secondary.clone()],
        )
        .unwrap();
        state.apply_display_model(&next).unwrap();

        let primary = state
            .display_records
            .iter()
            .find(|record| record.model.key == DisplayKey::Primary)
            .expect("primary record should remain");
        assert_eq!(primary.handle, Some(secondary_handle));
        assert!(primary.last_runtime_state.is_some());

        let old_primary_identity = state
            .display_records
            .iter()
            .find(|record| record.model.key == DisplayKey::Identity(primary_identity.clone()))
            .expect("old primary identity record should remain");
        assert_eq!(old_primary_identity.handle, Some(primary_handle));
        assert_eq!(old_primary_identity.model.live_display, Some(new_secondary));

        let old_secondary_identity = state
            .display_records
            .iter()
            .find(|record| record.model.key == secondary_key)
            .expect("old secondary identity record should remain");
        assert_eq!(old_secondary_identity.handle, None);
        assert!(!old_secondary_identity.model.runtime_open);
    }
}
