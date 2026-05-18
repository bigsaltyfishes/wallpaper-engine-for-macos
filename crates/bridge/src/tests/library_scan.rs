use std::fs;

use crate::library::scan;

#[test]
fn scanner_returns_supported_and_unsupported_entries() {
    let root = tempfile::tempdir().unwrap();
    let scene = root.path().join("100");
    let web = root.path().join("200");
    fs::create_dir_all(&scene).unwrap();
    fs::create_dir_all(&web).unwrap();
    fs::write(
        scene.join("project.json"),
        r#"{"type":"scene","title":"Scene One","preview":"preview.png"}"#,
    )
    .unwrap();
    fs::write(scene.join("preview.png"), b"png").unwrap();
    fs::write(
        web.join("project.json"),
        r#"{"type":"web","title":"Web One","preview":"preview.png"}"#,
    )
    .unwrap();

    let entries = scan(root.path()).unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].workshop_id, "100");
    assert_eq!(entries[1].workshop_id, "200");
    assert!(
        entries
            .iter()
            .any(|entry| entry.workshop_id == "100" && entry.supported)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.workshop_id == "200" && !entry.supported)
    );
}
