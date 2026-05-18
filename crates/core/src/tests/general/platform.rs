use crate::tests::support::platform::{primary_display_or_skip, window_tests_enabled_or_skip};

#[test]
pub fn case_primary_display() {
    let Some(display) = primary_display_or_skip() else {
        return;
    };
    assert_ne!(display.display_id, 0);
    assert_ne!(display.width, 0);
    assert_ne!(display.height, 0);
}

#[test]
pub fn case_wallpaper_window() {
    if !window_tests_enabled_or_skip("window") {
        return;
    }
    let Some(display) = primary_display_or_skip() else {
        return;
    };
    let mut window = crate::WallpaperWindow::builder(display)
        .placeholder_style(crate::PlaceholderStyle::default())
        .open()
        .expect("window should open");
    assert!(window.is_open());
    assert!(!window.metal_layer_ptr().is_null());
    window.close();
    assert!(!window.is_open());
    assert!(window.metal_layer_ptr().is_null());
}

#[test]
pub fn case_wallpaper_window_update_display() {
    if !window_tests_enabled_or_skip("window update") {
        return;
    }
    let Some(display) = primary_display_or_skip() else {
        return;
    };
    let mut window = crate::WallpaperWindow::builder(display.clone())
        .placeholder_style(crate::PlaceholderStyle::default())
        .open()
        .expect("window should open");

    let updated = crate::DisplayDesc::with_identity(
        display.display_id,
        display.identity.clone(),
        display.x,
        display.y,
        display.width,
        display.height,
        if display.scale_factor > 1.0 { 1.0 } else { 2.0 },
    );
    window
        .update_display(updated.clone())
        .expect("window update should succeed");
    assert!(window.is_open());
    assert!(!window.metal_layer_ptr().is_null());

    let native_state = window
        .native_state_for_tests()
        .expect("native state should exist");
    let scale_factor = if updated.scale_factor > 0.0 {
        updated.scale_factor
    } else {
        1.0
    };
    let point_width = f64::from(updated.width) / scale_factor;
    let point_height = f64::from(updated.height) / scale_factor;
    assert_close(native_state.window_x, f64::from(updated.x));
    assert_close(native_state.window_y, f64::from(updated.y));
    assert_close(native_state.window_width, point_width);
    assert_close(native_state.window_height, point_height);
    assert_close(native_state.content_view_x, 0.0);
    assert_close(native_state.content_view_y, 0.0);
    assert_close(native_state.content_view_width, point_width);
    assert_close(native_state.content_view_height, point_height);
    assert_close(native_state.metal_layer_x, 0.0);
    assert_close(native_state.metal_layer_y, 0.0);
    assert_close(native_state.metal_layer_width, point_width);
    assert_close(native_state.metal_layer_height, point_height);
    assert_close(native_state.metal_layer_contents_scale, scale_factor);
    assert_close(
        native_state.metal_layer_drawable_width,
        f64::from(updated.width),
    );
    assert_close(
        native_state.metal_layer_drawable_height,
        f64::from(updated.height),
    );

    window.close();
    assert!(!window.is_open());
    let error = window
        .update_display(display)
        .expect_err("closed window update should fail");
    match error {
        crate::EngineError::Platform(message) => {
            assert_eq!(message, "wallpaper window is already closed");
        }
        error => panic!("expected closed window platform error, got {error:?}"),
    }
}

fn assert_close(actual: f64, expected: f64) {
    let tolerance = 0.001;
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {actual} to be within {tolerance} of {expected}"
    );
}
