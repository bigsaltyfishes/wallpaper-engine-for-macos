use std::fs;

use crate::config::ConfigStore;

#[test]
fn corrupted_config_is_backed_up_and_defaults_are_loaded() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path()).unwrap();
    fs::write(root.path().join("config.toml"), b"not = [valid").unwrap();

    let load = ConfigStore::open(root.path().to_path_buf()).load().unwrap();

    assert!(load.config.monitors.is_empty());
    let backups = fs::read_dir(root.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("config.toml.corrupted-")
        })
        .count();
    assert_eq!(backups, 1);
}
