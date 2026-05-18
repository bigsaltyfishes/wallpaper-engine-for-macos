use std::fs;

use crate::project::SceneFile;

#[test]
pub fn case_scene_file_parse() {
    let scene = SceneFile::parse(
        r#"{
            "general": {
                "orthogonalprojection": { "width": 400, "height": 200 }
            },
            "objects": [
                { "id": 7, "name": "Layer", "image": "models/layer.json" }
            ]
        }"#,
    )
    .expect("scene should parse");

    assert_eq!(scene.objects().len(), 1);
    assert!(scene.scene().render_targets().contains_key("_rt_default"));
}

#[test]
pub fn case_load_project_rejects_unsafe_scene_entries() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");

    fs::write(
        &project_path,
        r#"{ "type": "scene", "file": "../outside.json" }"#,
    )
    .expect("project should write");
    assert!(SceneFile::load_project(&project_path, root.path()).is_err());

    let absolute_scene = root.path().join("absolute.json");
    fs::write(
        &project_path,
        format!(
            r#"{{ "type": "scene", "file": "{}" }}"#,
            absolute_scene.display()
        ),
    )
    .expect("project should write");
    assert!(SceneFile::load_project(&project_path, root.path()).is_err());
}

#[test]
pub fn case_load_project_rejects_asset_path_traversal() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    let scene_path = root.path().join("scene.json");
    fs::write(
        &project_path,
        r#"{ "type": "scene", "file": "scene.json" }"#,
    )
    .expect("project should write");
    fs::write(
        &scene_path,
        r#"{
            "objects": [
                { "id": 1, "name": "Layer", "image": "../outside_asset.json" }
            ]
        }"#,
    )
    .expect("scene should write");

    assert!(SceneFile::load_project(&project_path, root.path()).is_err());
}
