use crate::{project::SceneFile, tests::fixtures::loadable_scene_fixture};

pub struct RuntimeCase {
    pub id: &'static str,
}

impl RuntimeCase {
    pub fn run(self) {
        let fixture = loadable_scene_fixture(self.id);

        let scene = SceneFile::load_project(fixture.project_path(), fixture.assets_path())
            .expect("scene project should load");
        assert!(!scene.objects().is_empty());
    }
}
