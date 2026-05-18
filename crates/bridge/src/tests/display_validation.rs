use crate::config::{AppConfig, MonitorCfg, SerializedSelector};

#[test]
fn mirror_validation_rejects_self_target() {
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 2 },
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: None,
        mirror_target: None,
    });

    let result = config.validate_mirror_change(
        &SerializedSelector::LiveDisplayId { display_id: 2 },
        &SerializedSelector::LiveDisplayId { display_id: 2 },
    );

    assert!(result.is_err());
}
