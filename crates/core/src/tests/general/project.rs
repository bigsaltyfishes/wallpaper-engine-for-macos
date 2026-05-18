use std::fs;

use crate::project::{
    ProjectManifest, ProjectProperties, SceneSourceResolution, WallpaperProjectType,
};

#[test]
pub fn case_manifest_load_and_properties_override() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    fs::write(
        &project_path,
        r#"{
            "type": "scene",
            "file": "scenes/main.json",
            "workshopid": 123456,
            "dependencies": ["shared_textures", "workshop/123"],
            "general": {
                "properties": {
                    "timevarying": { "value": true },
                    "display": { "value": "1" }
                }
            }
        }"#,
    )
    .expect("project should write");

    let manifest = ProjectManifest::load(&project_path).expect("manifest should load");
    assert_eq!(manifest.project_type(), WallpaperProjectType::Scene);
    assert_eq!(manifest.file(), "scenes/main.json");
    assert_eq!(manifest.workshop_id(), "123456");
    assert_eq!(
        manifest.dependency_paths(root.path()),
        vec![
            root.path().join("shared_textures"),
            root.path().join("workshop/123")
        ]
    );

    let properties = ProjectProperties::load(&project_path).expect("properties should load");
    let updated = properties
        .apply_override(r#"{ "display": "2" }"#)
        .expect("override should apply");

    assert_eq!(properties.get("display").unwrap().as_string(), "1");
    assert_eq!(updated.get("display").unwrap().as_string(), "2");
    assert!(updated.get("timevarying").unwrap().as_bool());
}

#[test]
pub fn case_scene_source_rejects_unsafe_manifest_entries() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");

    fs::write(
        &project_path,
        r#"{ "type": "scene", "file": "../outside.json" }"#,
    )
    .expect("project should write");
    assert!(SceneSourceResolution::load(&project_path).is_err());

    let absolute_scene = root.path().join("absolute.json");
    fs::write(
        &project_path,
        format!(
            r#"{{ "type": "scene", "file": "{}" }}"#,
            absolute_scene.display()
        ),
    )
    .expect("project should write");
    assert!(SceneSourceResolution::load(&project_path).is_err());
}
