use crate::{config::WallpaperConfig, state::drafts::WallpaperOptionsDraft};

#[test]
fn cancel_restores_committed_wallpaper_config() {
    let committed = WallpaperConfig::new_for("100", "scene");
    let mut draft = WallpaperOptionsDraft::from_committed(committed.clone());

    draft.set_volume(0.25).unwrap();
    assert!(draft.is_dirty());

    draft.cancel();

    assert_eq!(draft.current(), &committed);
    assert!(!draft.is_dirty());
}
