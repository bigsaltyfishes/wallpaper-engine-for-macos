use crate::{BridgeWallpaperEntry, BridgeWallpaperKind, WallpaperBridge};

#[tokio::test]
async fn replacing_library_clears_selection_when_wallpaper_disappears() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("100", "Scene One", BridgeWallpaperKind::ProjectScene)
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    bridge
        .replace_library_for_test(vec![BridgeWallpaperEntry {
            id: "200".to_string(),
            title: "Scene Two".to_string(),
            kind: BridgeWallpaperKind::ProjectScene,
            supported: true,
            active: false,
            selected: false,
            preview_path: None,
        }])
        .await;

    assert_eq!(
        bridge
            .app_snapshot()
            .await
            .unwrap()
            .selected_wallpaper_id
            .as_deref(),
        None
    );
    assert!(
        !bridge
            .library_snapshot()
            .await
            .unwrap()
            .wallpapers
            .iter()
            .any(|entry| entry.selected)
    );
}
