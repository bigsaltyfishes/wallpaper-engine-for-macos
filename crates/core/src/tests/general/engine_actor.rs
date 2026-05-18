use wallpaper_core::{EngineError, WallpaperEngineConfig};

#[tokio::test(flavor = "current_thread")]
async fn actor_test_api_can_start_engine_without_display_assets() {
    let engine = wallpaper_core::WallpaperEngine::with_config(WallpaperEngineConfig::default())
        .expect("engine should initialize");

    engine
        .actor_ping_for_test()
        .await
        .expect("actor ping should succeed");
}

#[tokio::test(flavor = "current_thread")]
async fn actor_error_mapping_reports_closed_mailbox_for_test() {
    let error = wallpaper_core::WallpaperEngine::actor_closed_error_for_test().await;

    assert!(matches!(error, EngineError::Platform(_)));
    assert!(error.to_string().contains("engine actor"));
}

#[tokio::test(flavor = "current_thread")]
async fn actor_shell_uses_configured_display_model_for_test() {
    let external = wallpaper_core::DisplayIdentity {
        uuid: Some("external".to_string()),
        vendor_id: Some(1),
        model_id: Some(2),
        serial_number: Some(3),
        unit_number: Some(4),
        name: None,
    };
    let engine = wallpaper_core::WallpaperEngine::with_config(WallpaperEngineConfig {
        displays: vec![
            wallpaper_core::DisplayConfig {
                selector: wallpaper_core::DisplaySelector::Primary,
                window_active: true,
                wallpaper: None,
            },
            wallpaper_core::DisplayConfig {
                selector: wallpaper_core::DisplaySelector::Identity(external),
                window_active: true,
                wallpaper: None,
            },
        ],
    })
    .expect("engine should initialize");

    assert_eq!(
        engine
            .actor_display_record_count_for_test()
            .await
            .expect("actor display count should succeed"),
        2
    );
}

#[tokio::test(flavor = "current_thread")]
async fn actor_processes_test_messages_in_mailbox_order() {
    let engine = wallpaper_core::WallpaperEngine::with_config(WallpaperEngineConfig::default())
        .expect("engine should initialize");

    let first = engine.actor_sequence_for_test(1).await.unwrap();
    let second = engine.actor_sequence_for_test(2).await.unwrap();

    assert_eq!(first, 1);
    assert_eq!(second, 2);
}
