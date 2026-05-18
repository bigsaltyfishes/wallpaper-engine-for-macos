use crate::{BridgeErrorKind, BridgePropertyKind, BridgePropertyValue, WallpaperBridge};

fn assert_f64_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON,
        "expected {actual} to be within f64::EPSILON of {expected}"
    );
}

#[tokio::test]
async fn property_snapshot_exposes_raw_html_and_edit_dirty_state() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_scene_project_for_test(
            "100",
            "Scene",
            r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "enabled":{"type":"bool","text":"<b>Bold</b>","value":false}
            }}
        }"#,
        )
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let snapshot = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();

    assert_eq!(snapshot.properties.len(), 1);
    assert_eq!(snapshot.properties[0].kind, BridgePropertyKind::Bool);
    assert_eq!(snapshot.properties[0].label_html, "<b>Bold</b>");

    bridge
        .edit_property(
            "100".to_string(),
            "enabled".to_string(),
            BridgePropertyValue::Bool { value: true },
        )
        .await
        .unwrap();
    let edited = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(edited.properties[0].dirty);

    bridge
        .restore_property_default("100".to_string(), "enabled".to_string())
        .await
        .unwrap();
    let restored = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!restored.properties[0].dirty);
}

#[tokio::test]
async fn property_snapshot_exposes_slider_metadata_and_accepts_in_range_edit() {
    let bridge = WallpaperBridge::new_for_test();
    bridge.inject_scene_project_for_test(
        "100",
        "Scene",
        r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "size":{"type":"slider","text":"Size","value":10,"min":10,"max":20,"step":2,"precision":0}
            }}
        }"#,
    ).await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let snapshot = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();

    assert_eq!(snapshot.properties[0].kind, BridgePropertyKind::Slider);
    let metadata = snapshot.properties[0]
        .slider
        .as_ref()
        .expect("slider metadata should be exposed");
    assert_f64_close(metadata.min, 10.0);
    assert_f64_close(metadata.max, 20.0);
    assert_f64_close(metadata.step, 2.0);
    assert_eq!(metadata.precision, 0);

    bridge
        .edit_property(
            "100".to_string(),
            "size".to_string(),
            BridgePropertyValue::Number { value: 16.0 },
        )
        .await
        .unwrap();

    let edited = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(edited.properties[0].dirty);
}

#[tokio::test]
async fn invalid_property_edits_return_invalid_input_without_dirtying_draft() {
    let bridge = WallpaperBridge::new_for_test();
    bridge.inject_scene_project_for_test(
        "100",
        "Scene",
        r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "enabled":{"type":"bool","text":"Enabled","value":false},
                "amount":{"type":"slider","text":"Amount","value":10,"min":10,"max":20,"step":2,"precision":0},
                "tint":{"type":"color","text":"Tint","value":"0.1 0.2 0.3"},
                "choice":{"type":"combo","text":"Choice","value":"a","options":[{"label":"A","value":"a"},{"label":"B","value":"b"}]}
            }}
        }"#,
    ).await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let invalid_edits = [
        ("missing", BridgePropertyValue::Bool { value: true }),
        (
            "enabled",
            BridgePropertyValue::String {
                value: "true".into(),
            },
        ),
        ("amount", BridgePropertyValue::Number { value: f64::NAN }),
        (
            "amount",
            BridgePropertyValue::Number {
                value: f64::INFINITY,
            },
        ),
        ("amount", BridgePropertyValue::Number { value: 22.0 }),
        (
            "tint",
            BridgePropertyValue::ColorRgb {
                red: 1.2,
                green: 0.0,
                blue: 0.0,
            },
        ),
        (
            "tint",
            BridgePropertyValue::ColorRgb {
                red: f64::NAN,
                green: 0.0,
                blue: 0.0,
            },
        ),
        (
            "choice",
            BridgePropertyValue::String {
                value: "missing".into(),
            },
        ),
    ];

    for (property_id, value) in invalid_edits {
        let error = bridge
            .edit_property("100".to_string(), property_id.to_string(), value)
            .await
            .expect_err("invalid property edit should be rejected");
        assert_eq!(error.kind(), BridgeErrorKind::InvalidInput);
        let snapshot = bridge
            .wallpaper_options_snapshot("100".to_string())
            .await
            .unwrap();
        assert!(
            !snapshot.dirty,
            "invalid edit for {property_id} must not dirty the draft"
        );
        assert!(
            snapshot.properties.iter().all(|property| !property.dirty),
            "invalid edit for {property_id} must not dirty any property"
        );
    }
}

#[tokio::test]
async fn restore_unknown_property_returns_invalid_input_without_dirtying_draft() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_scene_project_for_test(
            "100",
            "Scene",
            r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "enabled":{"type":"bool","text":"Enabled","value":false}
            }}
        }"#,
        )
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let error = bridge
        .restore_property_default("100".to_string(), "missing".to_string())
        .await
        .expect_err("unknown property restore should be rejected");

    assert_eq!(error.kind(), BridgeErrorKind::InvalidInput);
    let snapshot = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!snapshot.dirty);
    assert!(snapshot.properties.iter().all(|property| !property.dirty));
}
