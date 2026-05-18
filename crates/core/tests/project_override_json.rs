use serde_json::Value;
use wallpaper_core::project::{ScenePropertyOverrideError, SerdeValudeExt};

#[test]
fn nested_override_json_is_flattened_with_underscores() {
    let flattened =
        serde_json::from_str::<Value>(r#"{"example":{"inner":true},"count":1.5,"label":"demo"}"#)
            .expect("valid override json")
            .flatten()
            .expect("flattening should succeed");
    let flattened = serde_json::to_string(&flattened).expect("flattened json should serialize");

    assert_eq!(
        flattened,
        r#"{"count":1.5,"example_inner":true,"label":"demo"}"#
    );
}

#[test]
fn null_override_is_encoded_as_empty_string() {
    let flattened = serde_json::from_str::<Value>(r#"{"text":null}"#)
        .expect("valid null override")
        .flatten()
        .expect("flattening should succeed");
    let flattened = serde_json::to_string(&flattened).expect("flattened json should serialize");
    assert_eq!(flattened, r#"{"text":""}"#);
}

#[test]
fn arrays_are_rejected() {
    let error = serde_json::from_str::<Value>(r#"{"broken":[1,2,3]}"#)
        .expect("valid override json")
        .flatten()
        .unwrap_err();
    assert!(matches!(
        error,
        ScenePropertyOverrideError::UnsupportedLeafType(_)
    ));
}
