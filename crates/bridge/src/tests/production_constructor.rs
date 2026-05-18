use crate::{BridgeError, WallpaperBridge};

#[test]
fn production_constructor_is_fallible() {
    assert!(std::ptr::fn_addr_eq(
        WallpaperBridge::new as fn() -> Result<WallpaperBridge, BridgeError>,
        WallpaperBridge::new as fn() -> Result<WallpaperBridge, BridgeError>,
    ));
}
