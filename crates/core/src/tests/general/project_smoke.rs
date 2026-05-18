use crate::{
    project::{ProjectManifest, WallpaperProjectType},
    tests::fixtures::project_path,
};

pub struct ProjectManifestCase {
    pub id: &'static str,
}

impl ProjectManifestCase {
    pub fn run(self) {
        let path = project_path(self.id);
        if !path.is_file() {
            return;
        }

        let manifest = ProjectManifest::load(path).expect("manifest should load");
        assert_eq!(manifest.project_type(), WallpaperProjectType::Scene);
    }
}
