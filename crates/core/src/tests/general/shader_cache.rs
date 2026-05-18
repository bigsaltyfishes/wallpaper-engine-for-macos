use std::fs;

use crate::{
    project::SceneDesc,
    render::{ShaderCacheDecision, ShaderCacheInputs},
};

#[test]
pub fn case_shader_cache_prepare() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    let pkg_path = root.path().join("scene.pkg");
    fs::write(&project_path, "{}").expect("project should write");
    fs::write(&pkg_path, "pkg").expect("pkg should write");

    let inputs = ShaderCacheInputs::builder("demo-scene", root.path().join("cache"))
        .project_json_path(&project_path)
        .scene_pkg_path(&pkg_path)
        .build()
        .expect("inputs should build");

    let cold = ShaderCacheDecision::prepare(&inputs).expect("cold cache should prepare");
    assert!(cold.purged_cache());

    let warm = ShaderCacheDecision::prepare(&inputs).expect("warm cache should prepare");
    assert!(!warm.purged_cache());
}

#[test]
pub fn case_shader_cache_purges_when_property_overrides_change() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    let pkg_path = root.path().join("scene.pkg");
    fs::write(&project_path, "{}").expect("project should write");
    fs::write(&pkg_path, "pkg").expect("pkg should write");

    let first = ShaderCacheInputs::builder("demo-scene", root.path().join("cache"))
        .project_json_path(&project_path)
        .scene_pkg_path(&pkg_path)
        .property_override_json(Some(r#"{"color":"red"}"#))
        .build()
        .expect("inputs should build");
    let second = ShaderCacheInputs::builder("demo-scene", root.path().join("cache"))
        .project_json_path(&project_path)
        .scene_pkg_path(&pkg_path)
        .property_override_json(Some(r#"{"color":"blue"}"#))
        .build()
        .expect("inputs should build");

    let cold = ShaderCacheDecision::prepare(&first).expect("cold cache should prepare");
    assert!(cold.purged_cache());

    let changed = ShaderCacheDecision::prepare(&second).expect("changed cache should prepare");
    assert!(changed.purged_cache());

    let warm = ShaderCacheDecision::prepare(&second).expect("warm cache should prepare");
    assert!(!warm.purged_cache());
}

#[test]
pub fn case_shader_cache_rejects_scene_id_path_escape() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    let pkg_path = root.path().join("scene.pkg");
    let cache_root = root.path().join("cache");
    let victim_path = root.path().join("victim").join("keep.txt");
    fs::create_dir_all(victim_path.parent().unwrap()).expect("victim dir should write");
    fs::write(&victim_path, "do not delete").expect("victim should write");
    fs::write(&project_path, "{}").expect("project should write");
    fs::write(&pkg_path, "pkg").expect("pkg should write");

    let error = ShaderCacheInputs::builder("../victim", &cache_root)
        .project_json_path(&project_path)
        .scene_pkg_path(&pkg_path)
        .force_refresh(true)
        .build()
        .expect_err("path traversal scene id should fail");

    assert!(matches!(error, crate::EngineError::InvalidInput(_)));
    assert!(victim_path.exists());

    for invalid in ["/absolute", "nested/path", "nested\\path", "."] {
        assert!(
            ShaderCacheInputs::builder(invalid, &cache_root)
                .project_json_path(&project_path)
                .scene_pkg_path(&pkg_path)
                .build()
                .is_err()
        );
    }
}

#[test]
pub fn case_shader_cache_ignores_non_scene_projects() {
    let root = tempfile::tempdir().expect("tempdir should exist");
    let project_path = root.path().join("project.json");
    fs::write(
        &project_path,
        r#"{"type":"video","file":"video.mp4","workshopid":"video-demo"}"#,
    )
    .expect("project should write");

    let scene = SceneDesc::builder(
        crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
        project_path.to_string_lossy(),
    )
    .assets_path(root.path().join("assets").to_string_lossy())
    .shader_cache_path(root.path().join("cache").to_string_lossy())
    .build()
    .expect("scene desc should build");

    let cache_path = scene
        .shader_cache_path()
        .expect("non-scene projects should not require scene.pkg metadata");

    assert_eq!(cache_path, None);
}
