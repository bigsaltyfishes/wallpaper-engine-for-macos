use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::project::SceneSourceResolution;

pub const WORKSHOP_ROOT: &str =
    "/Users/molyuu/Library/Application Support/Steam/steamapps/workshop/content/431960";
pub const ASSETS_ROOT: &str =
    "/Users/molyuu/Library/Application Support/Steam/steamapps/common/wallpaper_engine/assets";

pub fn project_path(id: &str) -> PathBuf {
    Path::new(WORKSHOP_ROOT).join(id).join("project.json")
}

pub fn assets_path() -> PathBuf {
    PathBuf::from(ASSETS_ROOT)
}

pub struct SmokeSceneFixture {
    _root: Option<tempfile::TempDir>,
    project_path: PathBuf,
    assets_path: PathBuf,
}

impl SmokeSceneFixture {
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    pub fn assets_path(&self) -> &Path {
        &self.assets_path
    }
}

pub fn loadable_scene_fixture(id: &str) -> SmokeSceneFixture {
    let project_path = project_path(id);
    let assets_path = assets_path();
    if project_path.is_file() && assets_path.is_dir() {
        match SceneSourceResolution::load(&project_path) {
            Ok(resolution) => {
                if let Some(scene_source) = resolution.scene_source() {
                    if scene_source.pkg_dir.join(&scene_source.pkg_entry).is_file() {
                        return SmokeSceneFixture {
                            _root: None,
                            project_path,
                            assets_path,
                        };
                    }

                    eprintln!(
                        "external smoke fixture {id} has no unpacked scene JSON at {}; using \
                         generated fixture",
                        scene_source.pkg_dir.join(&scene_source.pkg_entry).display()
                    );
                } else {
                    eprintln!(
                        "external smoke fixture {id} is not a scene project; using generated \
                         fixture"
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "external smoke fixture {id} could not resolve scene source: {error}; using \
                     generated fixture"
                );
            }
        }
    }

    let root = tempfile::Builder::new()
        .prefix(&format!("wallpaper-core-smoke-{id}-"))
        .tempdir()
        .expect("smoke fixture tempdir should exist");
    let assets_path = root.path().join("assets");
    fs::create_dir(&assets_path).expect("smoke fixture assets directory should write");

    let project_path = root.path().join("project.json");
    fs::write(
        &project_path,
        format!(
            r#"{{
                "type": "scene",
                "file": "scene.json",
                "workshopid": "{id}"
            }}"#
        ),
    )
    .expect("smoke fixture project should write");
    fs::write(
        root.path().join("scene.json"),
        r#"{
            "general": {
                "orthogonalprojection": { "width": 1280, "height": 720 }
            },
            "objects": [
                { "id": 1, "name": "Smoke Layer" }
            ]
        }"#,
    )
    .expect("smoke fixture scene should write");

    SmokeSceneFixture {
        _root: Some(root),
        project_path,
        assets_path,
    }
}
