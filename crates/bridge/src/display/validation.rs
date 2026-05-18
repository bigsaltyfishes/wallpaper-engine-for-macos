use crate::{
    api::BridgeError,
    config::{AppConfig, SerializedSelector},
};

impl AppConfig {
    /// # Errors
    ///
    /// Returns an error when the target mirrors the source display or would
    /// create a mirror cycle.
    pub fn validate_mirror_change(
        &self,
        selector: &SerializedSelector,
        target: &SerializedSelector,
    ) -> Result<(), BridgeError> {
        if selector == target {
            return Err(BridgeError::invalid_input("a display cannot mirror itself"));
        }

        let mut current = target.clone();
        for _ in 0..self.monitors.len() {
            let Some(row) = self.monitors.iter().find(|row| row.selector == current) else {
                return Ok(());
            };
            if row.mode != "mirror" {
                return Ok(());
            }
            let Some(next) = row.mirror_target.clone() else {
                return Ok(());
            };
            if &next == selector {
                return Err(BridgeError::invalid_input(
                    "mirror mode cannot create a cycle",
                ));
            }
            current = next;
        }

        Err(BridgeError::invalid_input(
            "mirror mode cannot create a cycle",
        ))
    }
}
