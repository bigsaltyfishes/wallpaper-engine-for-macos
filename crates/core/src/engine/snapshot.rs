use std::sync::Arc;

use super::DisplaySnapshotEntry;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineSnapshot {
    pub displays: Vec<DisplaySnapshotEntry>,
}

#[derive(Debug)]
pub struct EngineSnapshotPublisher {
    snapshot: arc_swap::ArcSwap<EngineSnapshot>,
}

impl EngineSnapshotPublisher {
    #[allow(clippy::single_call_fn)]
    pub fn new(snapshot: EngineSnapshot) -> Self {
        Self {
            snapshot: arc_swap::ArcSwap::from_pointee(snapshot),
        }
    }

    pub fn load(&self) -> Arc<EngineSnapshot> {
        self.snapshot.load_full()
    }

    pub fn publish(&self, snapshot: EngineSnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DisplayDesc, DisplayIdentity};

    #[test]
    fn publisher_returns_initial_snapshot() {
        let publisher = EngineSnapshotPublisher::new(EngineSnapshot::default());

        assert_eq!(publisher.load().displays, Vec::new());
    }

    #[test]
    fn publisher_returns_latest_snapshot() {
        let identity = DisplayIdentity {
            uuid: Some("display-uuid".to_string()),
            vendor_id: Some(10),
            model_id: Some(20),
            serial_number: Some(30),
            unit_number: Some(1),
            name: Some("Studio Display".to_string()),
        };
        let display = DisplayDesc::with_identity(9, identity.clone(), 0, 0, 1920, 1080, 1.0);
        let entry = DisplaySnapshotEntry {
            identity,
            desc: display,
            handle: None,
            window_active: true,
            assignment: None,
        };
        let publisher = EngineSnapshotPublisher::new(EngineSnapshot::default());

        publisher.publish(EngineSnapshot {
            displays: vec![entry.clone()],
        });

        assert_eq!(publisher.load().displays, vec![entry]);
    }
}
