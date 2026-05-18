use crate::{project::SceneFile, tests::fixtures::loadable_scene_fixture};

pub struct RenderGraphCase {
    pub id: &'static str,
}

impl RenderGraphCase {
    pub fn run(self) {
        let fixture = loadable_scene_fixture(self.id);

        let scene = SceneFile::load_project(fixture.project_path(), fixture.assets_path())
            .expect("scene project should load");
        let render_targets = scene.scene().render_targets();
        assert!(!render_targets.is_empty());
        assert!(
            render_targets
                .values()
                .all(|target| target.width > 0 && target.height > 0)
        );
    }
}
