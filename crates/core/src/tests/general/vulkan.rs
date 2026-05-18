use crate::tests::support::platform::{primary_display_or_skip, window_tests_enabled_or_skip};

#[test]
pub fn case_vulkan_surface() {
    if !window_tests_enabled_or_skip("vulkan surface") {
        return;
    }
    let Some(display) = primary_display_or_skip() else {
        return;
    };
    let mut window = crate::WallpaperWindow::builder(display.clone())
        .open()
        .expect("window should open");

    let config = crate::render::RenderSurfaceConfig::builder(window.metal_layer_ptr())
        .extent(display.width, display.height)
        .build()
        .expect("surface config should build");
    // SAFETY: `window` owns a retained CAMetalLayer for the duration of the
    // render surface and is closed only after the surface is shut down.
    let mut surface = unsafe { crate::render::VulkanRenderSurface::initialize(config) }
        .expect("surface should initialize");

    assert!(surface.is_ready());
    surface
        .clear_and_present([0.1, 0.2, 0.3, 1.0])
        .expect("clear should present");
    surface.shutdown();
    window.close();
}
