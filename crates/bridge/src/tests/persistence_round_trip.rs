use crate::config::{AppConfig, ConfigStore, MonitorCfg, SerializedSelector};

#[test]
fn app_config_round_trips_monitor_assignment() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.ui.filter.scene = false;
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::Primary,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("3470764447".to_string()),
        mirror_target: None,
    });

    store.save_app_config(&config).unwrap();

    let reloaded = ConfigStore::open(root.path().to_path_buf())
        .load()
        .unwrap()
        .config;
    assert_eq!(reloaded, config);
}
