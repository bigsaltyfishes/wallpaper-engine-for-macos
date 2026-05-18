use crate::{
    project::SceneSourceResolution,
    tests::fixtures::{assets_path, project_path},
};

pub struct ResourceCase {
    pub id: &'static str,
}

impl ResourceCase {
    pub fn run(self) {
        let project = project_path(self.id);
        let assets = assets_path();
        if !project.is_file() || !assets.is_dir() {
            return;
        }

        let resolution =
            SceneSourceResolution::load(&project).expect("scene source should resolve");
        assert!(resolution.scene_source().is_some());
    }
}
