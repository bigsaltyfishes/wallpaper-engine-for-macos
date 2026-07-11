use std::path::{Path, PathBuf};

use objc2::{
    ClassType, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::AnyObject,
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSScreen, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
#[cfg(not(test))]
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSInteger, NSPoint, NSRect, NSSize, NSThread};
use objc2_quartz_core::CAMetalLayer;

use crate::{DisplayDesc, EngineError};

define_class!(
    // SAFETY: The subclass only overrides frame constraint behavior and does
    // not add ivars or custom memory management.
    #[unsafe(super = NSWindow)]
    #[thread_kind = MainThreadOnly]
    struct WallpaperDesktopWindow;

    impl WallpaperDesktopWindow {
        #[unsafe(method(constrainFrameRect:toScreen:))]
        fn constrain_frame_rect_to_screen(
            &self,
            frame: NSRect,
            _screen: Option<&NSScreen>,
        ) -> NSRect {
            frame
        }
    }
);

impl WallpaperDesktopWindow {
    #[allow(clippy::single_call_fn)]
    unsafe fn init_with_content_rect(
        mtm: MainThreadMarker,
        frame: NSRect,
        style_mask: NSWindowStyleMask,
        backing: NSBackingStoreType,
        defer: bool,
    ) -> Retained<Self> {
        unsafe {
            msg_send![
                Self::alloc(mtm),
                initWithContentRect: frame,
                styleMask: style_mask,
                backing: backing,
                defer: defer
            ]
        }
    }
}

/// Initial background color for a wallpaper window before renderer content is
/// visible.
#[derive(Clone, Copy, Debug)]
pub struct PlaceholderStyle {
    /// Red channel in the range AppKit accepts for sRGB colors.
    #[allow(clippy::doc_markdown)]
    pub red: f64,
    /// Green channel in the range AppKit accepts for sRGB colors.
    #[allow(clippy::doc_markdown)]
    pub green: f64,
    /// Blue channel in the range AppKit accepts for sRGB colors.
    #[allow(clippy::doc_markdown)]
    pub blue: f64,
    /// Alpha channel in the range AppKit accepts for sRGB colors.
    #[allow(clippy::doc_markdown)]
    pub alpha: f64,
}

impl Default for PlaceholderStyle {
    fn default() -> Self {
        Self {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            alpha: 1.0,
        }
    }
}

/// Borderless desktop-level window that hosts a `CAMetalLayer`.
///
/// This type exists for focused window/surface tests and for future Rust-native
/// rendering work. The active scene renderer currently owns its own equivalent
/// window through the statically linked native bridge.
pub struct WallpaperWindow {
    display: DisplayDesc,
    handle: Option<WindowHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MouseButtonState {
    pub button: u32,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MouseButtons {
    mask: u64,
}

impl MouseButtons {
    #[must_use]
    #[allow(clippy::single_call_fn)]
    pub(crate) fn from_mask(mask: u64) -> Self {
        Self { mask }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn states(self) -> Vec<MouseButtonState> {
        (0..32)
            .map(|button| MouseButtonState {
                button,
                pressed: (self.mask & (1u64 << button)) != 0,
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn mask(self) -> u64 {
        self.mask
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MouseButtonEdges {
    down: MouseButtons,
    pressed: MouseButtons,
    released: MouseButtons,
}

impl MouseButtonEdges {
    #[must_use]
    pub(crate) fn from_level_state(buttons: MouseButtons) -> Self {
        Self {
            down: buttons,
            pressed: buttons,
            released: MouseButtons::default(),
        }
    }

    #[allow(clippy::single_call_fn)]
    #[must_use]
    pub(crate) fn from_masks(down: u64, pressed: u64, released: u64) -> Self {
        Self {
            down: MouseButtons::from_mask(down),
            pressed: MouseButtons::from_mask(pressed),
            released: MouseButtons::from_mask(released),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn down(self) -> MouseButtons {
        self.down
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn states(self) -> Vec<MouseButtonState> {
        let transitions = self.pressed.mask | self.released.mask;
        if transitions == 0 {
            return self.down.states();
        }

        (0..32)
            .flat_map(|button| {
                let mask = 1u64 << button;
                let mut states = Vec::with_capacity(2);
                if (self.pressed.mask & mask) != 0 {
                    states.push(MouseButtonState {
                        button,
                        pressed: true,
                    });
                }
                if (self.released.mask & mask) != 0 {
                    states.push(MouseButtonState {
                        button,
                        pressed: false,
                    });
                }
                if states.is_empty() {
                    states.push(MouseButtonState {
                        button,
                        pressed: (self.down.mask & mask) != 0,
                    });
                }
                states
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn transitions(self) -> Vec<MouseButtonState> {
        (0..32)
            .flat_map(|button| {
                let mask = 1u64 << button;
                let mut states = Vec::with_capacity(2);
                if (self.pressed.mask & mask) != 0 {
                    states.push(MouseButtonState {
                        button,
                        pressed: true,
                    });
                }
                if (self.released.mask & mask) != 0 {
                    states.push(MouseButtonState {
                        button,
                        pressed: false,
                    });
                }
                states
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub(crate) struct MouseButtonTracker {
    down: u64,
    pressed: u64,
    released: u64,
}

impl MouseButtonTracker {
    #[allow(clippy::single_call_fn)]
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_button(&mut self, button: u32, pressed: bool) {
        if button > 31 {
            return;
        }
        let mask = 1u64 << button;
        if pressed {
            if (self.down & mask) == 0 {
                self.down |= mask;
                self.pressed |= mask;
            }
            return;
        }

        if (self.down & mask) != 0 {
            self.down &= !mask;
            self.released |= mask;
        }
    }

    pub(crate) fn sync_down_mask(&mut self, mask: u64) {
        let next = mask & u64::from(u32::MAX);
        self.pressed |= next & !self.down;
        self.released |= self.down & !next;
        self.down = next;
    }

    #[must_use]
    pub(crate) fn consume_edges(&mut self) -> MouseButtonEdges {
        let edges = MouseButtonEdges::from_masks(self.down, self.pressed, self.released);
        self.pressed = 0;
        self.released = 0;
        edges
    }
}

#[cfg(not(test))]
pub(crate) struct MouseEventMonitor {
    monitor: SendPtr,
    #[allow(dead_code)]
    block: block2::RcBlock<dyn Fn(std::ptr::NonNull<NSEvent>)>,
}

#[cfg(test)]
pub(crate) struct MouseEventMonitor;

#[cfg(not(test))]
impl MouseEventMonitor {
    #[allow(clippy::single_call_fn)]
    pub(crate) fn new<F>(handler: F) -> Option<Self>
    where
        F: Fn(MouseButtonState) + 'static,
    {
        let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
            // SAFETY: AppKit invokes the monitor block with a valid NSEvent for
            // the event mask supplied when registering the monitor.
            let event = unsafe { event.as_ref() };
            let Some(state) = mouse_button_state_from_event(event) else {
                return;
            };
            handler(state);
        });

        let retained_monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            mouse_button_event_mask(),
            &block,
        )?;
        let monitor = SendPtr(Retained::as_ptr(&retained_monitor).cast_mut().cast());
        std::mem::forget(retained_monitor);

        Some(Self { monitor, block })
    }
}

#[cfg(not(test))]
impl Drop for MouseEventMonitor {
    fn drop(&mut self) {
        let monitor = self.monitor.0 as usize;
        run_on_main_thread(move || unsafe {
            let object = Retained::from_raw((monitor as *mut std::ffi::c_void).cast::<AnyObject>())
                .expect("mouse event monitor token should not be null");
            NSEvent::removeMonitor(&object);
        });
    }
}

// SAFETY: The monitor token is retained and only removed on the AppKit main
// thread in Drop. The retained block must stay alive for AppKit callbacks, but
// it is not invoked by Rust after construction.
#[cfg(not(test))]
unsafe impl Send for MouseEventMonitor {}
// SAFETY: The wrapper stores Objective-C tokens opaquely and all Objective-C
// interaction is dispatched to the main thread.
#[cfg(not(test))]
unsafe impl Sync for MouseEventMonitor {}

#[cfg(not(test))]
#[must_use]
#[allow(clippy::single_call_fn)]
fn mouse_button_event_mask() -> NSEventMask {
    NSEventMask::LeftMouseDown
        | NSEventMask::LeftMouseUp
        | NSEventMask::RightMouseDown
        | NSEventMask::RightMouseUp
        | NSEventMask::OtherMouseDown
        | NSEventMask::OtherMouseUp
}

#[cfg(not(test))]
#[must_use]
#[allow(clippy::single_call_fn)]
fn mouse_button_state_from_event(event: &NSEvent) -> Option<MouseButtonState> {
    let event_type = event.r#type();
    let pressed = if event_type == NSEventType::LeftMouseDown
        || event_type == NSEventType::RightMouseDown
        || event_type == NSEventType::OtherMouseDown
    {
        true
    } else if event_type == NSEventType::LeftMouseUp
        || event_type == NSEventType::RightMouseUp
        || event_type == NSEventType::OtherMouseUp
    {
        false
    } else {
        return None;
    };

    let button = match event_type {
        NSEventType::LeftMouseDown | NSEventType::LeftMouseUp => 0,
        NSEventType::RightMouseDown | NSEventType::RightMouseUp => 1,
        _ => u32::try_from(event.buttonNumber()).ok()?,
    };

    Some(MouseButtonState { button, pressed })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedMousePosition {
    pub x: f64,
    pub y: f64,
}

impl NormalizedMousePosition {
    #[must_use]
    #[allow(clippy::single_call_fn)]
    pub fn from_window_point(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
            return None;
        }
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        Some(Self {
            x: (x / width).clamp(0.0, 1.0),
            y: (1.0 - (y / height)).clamp(0.0, 1.0),
        })
    }
}

impl WallpaperWindow {
    /// Starts building a wallpaper window for `display`.
    #[must_use]
    pub fn builder(display: DisplayDesc) -> WallpaperWindowBuilder {
        WallpaperWindowBuilder {
            display,
            style: PlaceholderStyle::default(),
        }
    }

    /// Closes the native window if it is still open.
    pub fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            MainThread::dispatch(move || unsafe {
                handle.close_on_main();
            });
        }
    }

    /// Updates the native window, content view, and Metal layer to match
    /// `display`.
    ///
    /// # Errors
    ///
    /// Returns an error if the display geometry is invalid or the window has
    /// been closed.
    pub fn update_display(&mut self, display: DisplayDesc) -> Result<(), EngineError> {
        if display.display_id == 0 {
            return Err(EngineError::InvalidInput(
                "display_id must be non-zero".to_string(),
            ));
        }
        if display.width == 0 || display.height == 0 {
            return Err(EngineError::InvalidInput(
                "display dimensions must be non-zero".to_string(),
            ));
        }
        let Some(handle) = self.handle.as_ref() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let display_for_update = display.clone();
        let handle = handle.clone_for_main_thread();
        MainThread::dispatch(move || unsafe {
            handle.update_display(&display_for_update);
        });
        self.display = display;
        Ok(())
    }

    /// Replaces the placeholder Metal view with a web content view and loads
    /// a local HTML entry file.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be represented for Foundation,
    /// WebKit is unavailable, or the window has been closed.
    pub fn install_web_view(
        &mut self,
        html_path: &Path,
        initial_properties: Option<&wallpaper_web::Properties>,
    ) -> Result<(), EngineError> {
        let Some(handle) = self.handle.as_mut() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let html_path = html_path.to_path_buf();
        let read_access_root = PathBuf::from("/");
        let initial_properties = initial_properties.cloned();
        let handle_ref = handle.clone_for_main_thread();
        let web_view_ptr = MainThread::dispatch(move || unsafe {
            handle_ref
                .install_web_view(&html_path, &read_access_root, initial_properties.as_ref())
                .map_err(|e| match e {
                    wallpaper_web::WebError::InvalidInput(m) => EngineError::InvalidInput(m),
                    wallpaper_web::WebError::Platform(m) => EngineError::Platform(m),
                })
        })?;

        let new_content_view =
            MainThread::dispatch(move || unsafe { MainThread::retain_from_ptr(web_view_ptr) })?;
        let old_content_view = std::mem::replace(&mut handle.content_view, new_content_view);
        MainThread::dispatch(move || unsafe {
            old_content_view.release();
        });
        Ok(())
    }

    pub(crate) fn web_audio_dispatcher(
        &self,
    ) -> Result<wallpaper_web::AudioDispatcher, EngineError> {
        let Some(handle) = self.handle.as_ref() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let content_view = handle.content_view.as_ptr().cast::<std::ffi::c_void>();
        unsafe { wallpaper_web::AudioDispatcher::retain(wallpaper_web::ObjcPtr::new(content_view)) }
            .map_err(|e| match e {
                wallpaper_web::WebError::InvalidInput(m) => EngineError::InvalidInput(m),
                wallpaper_web::WebError::Platform(m) => EngineError::Platform(m),
            })
    }

    pub(crate) fn web_property_dispatcher(
        &self,
    ) -> Result<wallpaper_web::PropertyDispatcher, EngineError> {
        let Some(handle) = self.handle.as_ref() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let content_view = handle.content_view.as_ptr().cast::<std::ffi::c_void>();
        unsafe {
            wallpaper_web::PropertyDispatcher::retain(wallpaper_web::ObjcPtr::new(content_view))
        }
        .map_err(|e| match e {
            wallpaper_web::WebError::InvalidInput(m) => EngineError::InvalidInput(m),
            wallpaper_web::WebError::Platform(m) => EngineError::Platform(m),
        })
    }

    /// Returns whether this Rust-owned window still has a native handle.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.handle.is_some()
    }

    /// Replaces the `CAMetalLayer` under the existing `NSView` and returns the
    /// new layer's raw pointer, cast to `*mut c_void`. Used during
    /// display-reconfigure transactions: the returned pointer is passed to
    /// `OweScene::finish_surface_reconfigure`.
    ///
    /// # Errors
    ///
    /// Returns an error if the display geometry is invalid or the window has
    /// been closed.
    ///
    /// The `NSWindow` and `NSView` are retained. The old `CAMetalLayer` is
    /// released after the new layer is installed.
    pub fn update_layer(
        &mut self,
        display: DisplayDesc,
    ) -> Result<*mut std::ffi::c_void, EngineError> {
        if display.display_id == 0 {
            return Err(EngineError::InvalidInput(
                "display_id must be non-zero".to_string(),
            ));
        }
        if display.width == 0 || display.height == 0 {
            return Err(EngineError::InvalidInput(
                "display dimensions must be non-zero".to_string(),
            ));
        }
        let Some(handle) = self.handle.as_mut() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let handle_ref = handle.clone_for_main_thread();
        let display_clone = display.clone();
        let SendPtr(new_layer_ptr) = MainThread::dispatch(move || unsafe {
            handle_ref.replace_metal_layer(&display_clone)
        })?;

        // Swap the stored MainThread wrapper to track the new layer.
        let ptr_for_retain = SendPtr(new_layer_ptr);
        let old_metal_layer = std::mem::replace(
            &mut handle.metal_layer,
            MainThread::dispatch(move || unsafe { MainThread::retain_from_ptr(ptr_for_retain) })?,
        );
        MainThread::dispatch(move || unsafe {
            old_metal_layer.release();
        });

        self.display = display;
        Ok(new_layer_ptr)
    }

    /// Returns the retained `CAMetalLayer` pointer used by Vulkan/MoltenVK.
    ///
    /// The pointer is null after close. Callers must not outlive the returned
    /// pointer beyond the `WallpaperWindow` that owns it.
    #[must_use]
    pub fn metal_layer_ptr(&self) -> *mut std::ffi::c_void {
        self.handle.as_ref().map_or(std::ptr::null_mut(), |handle| {
            handle.metal_layer.as_ptr().cast::<std::ffi::c_void>()
        })
    }

    #[cfg(test)]
    /// # Errors
    ///
    /// Returns an error if the window has been closed.
    pub fn native_state_for_tests(&self) -> Result<WallpaperWindowNativeState, EngineError> {
        let Some(handle) = self.handle.as_ref() else {
            return Err(EngineError::Platform(
                "wallpaper window is already closed".to_string(),
            ));
        };

        let handle = handle.clone_for_main_thread();
        Ok(MainThread::dispatch(move || unsafe {
            handle.native_state()
        }))
    }
}

#[cfg(test)]
pub struct WallpaperWindowNativeState {
    pub window_x: f64,
    pub window_y: f64,
    pub window_width: f64,
    pub window_height: f64,
    pub content_view_x: f64,
    pub content_view_y: f64,
    pub content_view_width: f64,
    pub content_view_height: f64,
    pub metal_layer_x: f64,
    pub metal_layer_y: f64,
    pub metal_layer_width: f64,
    pub metal_layer_height: f64,
    pub metal_layer_contents_scale: f64,
    pub metal_layer_drawable_width: f64,
    pub metal_layer_drawable_height: f64,
}

impl Drop for WallpaperWindow {
    fn drop(&mut self) {
        self.close();
    }
}

/// Builder for a Rust-owned wallpaper window.
pub struct WallpaperWindowBuilder {
    display: DisplayDesc,
    style: PlaceholderStyle,
}

impl WallpaperWindowBuilder {
    /// Sets the placeholder color used before content is presented.
    #[must_use]
    pub fn placeholder_style(mut self, style: PlaceholderStyle) -> Self {
        self.style = style;
        self
    }

    /// Opens the native window.
    ///
    /// # Errors
    ///
    /// Returns an error if the display geometry is invalid or `AppKit` window
    /// creation fails.
    ///
    /// # Panics
    ///
    /// Panics only if main-thread dispatch does not return a result.
    pub fn open(self) -> Result<WallpaperWindow, EngineError> {
        if self.display.display_id == 0 {
            return Err(EngineError::InvalidInput(
                "display_id must be non-zero".to_string(),
            ));
        }
        if self.display.width == 0 || self.display.height == 0 {
            return Err(EngineError::InvalidInput(
                "display dimensions must be non-zero".to_string(),
            ));
        }

        MainThread::dispatch(|| unsafe {
            objc2::rc::autoreleasepool(|_| {
                let maker = MainThreadMarker::new().unwrap();
                let scale_factor = if self.display.scale_factor > 0.0 {
                    self.display.scale_factor
                } else {
                    1.0
                };
                let point_width = f64::from(self.display.width) / scale_factor;
                let point_height = f64::from(self.display.height) / scale_factor;
                let frame = NSRect::new(
                    NSPoint::new(f64::from(self.display.x), f64::from(self.display.y)),
                    NSSize::new(point_width, point_height),
                );
                let content_frame =
                    NSRect::new(NSPoint::ZERO, NSSize::new(point_width, point_height));

                let application = NSApplication::sharedApplication(maker);
                application.finishLaunching();

                let wallpaper_window = WallpaperDesktopWindow::init_with_content_rect(
                    maker,
                    frame,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                );
                let window = wallpaper_window.as_super();
                window.setReleasedWhenClosed(false);

                let window_level =
                    CGWindowLevelForKey(CGWindowLevelKey::DesktopWindowLevelKey) as NSInteger;
                let window_color = NSColor::colorWithSRGBRed_green_blue_alpha(
                    self.style.red,
                    self.style.green,
                    self.style.blue,
                    self.style.alpha,
                );

                window.setLevel(window_level);
                window.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::Stationary
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::IgnoresCycle,
                );
                window.setOpaque(true);
                window.setHasShadow(false);
                window.setMovable(false);
                window.setRestorable(false);
                window.setIgnoresMouseEvents(true);
                window.setHidesOnDeactivate(false);
                window.setExcludedFromWindowsMenu(true);
                window.setAnimationBehavior(NSWindowAnimationBehavior::None);
                window.setBackgroundColor(Some(&window_color));

                let content_view = NSView::initWithFrame(NSView::alloc(maker), content_frame);
                content_view.setWantsLayer(true);

                let metal_layer = self.display.build_metal_layer(&self.style);

                content_view.setLayer(Some(&metal_layer));
                window.setContentView(Some(&content_view));

                let window_handle = MainThread::retain(window, "NSWindow")?;
                let metal_layer_handle = match MainThread::retain(&metal_layer, "CAMetalLayer") {
                    Ok(handle) => handle,
                    Err(error) => {
                        window_handle.release();
                        return Err(error);
                    }
                };
                let content_view_handle = match MainThread::retain(&content_view, "NSView") {
                    Ok(handle) => handle,
                    Err(error) => {
                        window_handle.release();
                        metal_layer_handle.release();
                        return Err(error);
                    }
                };

                window.orderFrontRegardless();

                Ok(WallpaperWindow {
                    display: self.display,
                    handle: Some(WindowHandle {
                        window: window_handle,
                        content_view: content_view_handle,
                        metal_layer: metal_layer_handle,
                    }),
                })
            })
        })
    }
}

/// A raw pointer wrapper that is `Send`.
///
/// SAFETY: The pointer originates from an Objective-C object that is
/// reference-counted and thread-safe at the ARC level. We only transport the
/// pointer value across the dispatch boundary; actual dereferences happen
/// exclusively on the main thread.
#[derive(Clone, Copy)]
struct SendPtr(*mut std::ffi::c_void);
unsafe impl Send for SendPtr {}

struct MainThreadDispatchContext<F, R> {
    body: Option<F>,
    result: Option<std::thread::Result<R>>,
}

#[allow(clippy::single_call_fn)]
extern "C" fn invoke_main_thread_body<F, R>(context: *mut std::ffi::c_void)
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    let context = unsafe { &mut *context.cast::<MainThreadDispatchContext<F, R>>() };
    let body = context
        .body
        .take()
        .expect("main-thread body should run exactly once");
    context.result = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)));
}

struct WindowHandle {
    window: MainThread,
    content_view: MainThread,
    metal_layer: MainThread,
}

impl WindowHandle {
    fn clone_for_main_thread(&self) -> WindowHandleRef {
        WindowHandleRef {
            window: self.window.as_ptr(),
            content_view: self.content_view.as_ptr(),
            metal_layer: self.metal_layer.as_ptr(),
        }
    }

    unsafe fn close_on_main(self) {
        debug_assert!(NSThread::isMainThread_class());
        let window = unsafe { &*(self.window.as_ptr().cast::<NSWindow>()) };
        window.orderOut(None);
        window.close();
        unsafe {
            self.window.release();
            self.content_view.release();
            self.metal_layer.release();
        }
    }
}

struct WindowHandleRef {
    window: *mut AnyObject,
    content_view: *mut AnyObject,
    metal_layer: *mut AnyObject,
}

// SAFETY: These raw pointers are borrowed from an open `WallpaperWindow` and
// are only used inside synchronous `run_on_main_sync` operations. Safe Rust
// borrowing prevents the owner from being closed or dropped while the borrowed
// pointers are in flight.
unsafe impl Send for WindowHandleRef {}

impl WindowHandleRef {
    #[cfg(test)]
    unsafe fn native_state(self) -> WallpaperWindowNativeState {
        debug_assert!(NSThread::isMainThread_class());
        let window = unsafe { &*(self.window.cast::<NSWindow>()) };
        let content_view = unsafe { &*(self.content_view.cast::<NSView>()) };
        let metal_layer = unsafe { &*(self.metal_layer.cast::<CAMetalLayer>()) };

        let window_frame = window.frame();
        let content_view_frame = content_view.frame();
        let metal_layer_frame = metal_layer.frame();
        let drawable_size = metal_layer.drawableSize();

        WallpaperWindowNativeState {
            window_x: window_frame.origin.x,
            window_y: window_frame.origin.y,
            window_width: window_frame.size.width,
            window_height: window_frame.size.height,
            content_view_x: content_view_frame.origin.x,
            content_view_y: content_view_frame.origin.y,
            content_view_width: content_view_frame.size.width,
            content_view_height: content_view_frame.size.height,
            metal_layer_x: metal_layer_frame.origin.x,
            metal_layer_y: metal_layer_frame.origin.y,
            metal_layer_width: metal_layer_frame.size.width,
            metal_layer_height: metal_layer_frame.size.height,
            metal_layer_contents_scale: metal_layer.contentsScale(),
            metal_layer_drawable_width: drawable_size.width,
            metal_layer_drawable_height: drawable_size.height,
        }
    }

    unsafe fn update_display(self, display: &DisplayDesc) {
        debug_assert!(NSThread::isMainThread_class());
        let scale_factor = if display.scale_factor > 0.0 {
            display.scale_factor
        } else {
            1.0
        };
        let point_width = f64::from(display.width) / scale_factor;
        let point_height = f64::from(display.height) / scale_factor;
        let frame = NSRect::new(
            NSPoint::new(f64::from(display.x), f64::from(display.y)),
            NSSize::new(point_width, point_height),
        );
        let content_frame = NSRect::new(NSPoint::ZERO, NSSize::new(point_width, point_height));
        let drawable_size = NSSize::new(f64::from(display.width), f64::from(display.height));

        let window = unsafe { &*(self.window.cast::<NSWindow>()) };
        let content_view = unsafe { &*(self.content_view.cast::<NSView>()) };
        let metal_layer = unsafe { &*(self.metal_layer.cast::<CAMetalLayer>()) };

        window.setFrame_display(frame, true);
        content_view.setFrame(content_frame);
        metal_layer.setFrame(content_frame);
        metal_layer.setContentsScale(scale_factor);
        metal_layer.setDrawableSize(drawable_size);
    }

    #[allow(clippy::unnecessary_wraps)]
    unsafe fn replace_metal_layer(self, display: &DisplayDesc) -> Result<SendPtr, EngineError> {
        debug_assert!(NSThread::isMainThread_class());

        let new_layer = display.build_metal_layer(&PlaceholderStyle::default());

        // Install on the NSView.
        let content_view = unsafe { &*(self.content_view.cast::<NSView>()) };
        content_view.setLayer(Some(&new_layer));
        content_view.setWantsLayer(true);

        // Reshape window/view geometry.
        let scale_factor = if display.scale_factor > 0.0 {
            display.scale_factor
        } else {
            1.0
        };
        let point_width = f64::from(display.width) / scale_factor;
        let point_height = f64::from(display.height) / scale_factor;
        let frame = NSRect::new(
            NSPoint::new(f64::from(display.x), f64::from(display.y)),
            NSSize::new(point_width, point_height),
        );
        let content_frame = NSRect::new(NSPoint::ZERO, NSSize::new(point_width, point_height));
        let drawable_size = NSSize::new(f64::from(display.width), f64::from(display.height));

        let window = unsafe { &*(self.window.cast::<NSWindow>()) };
        window.setFrame_display(frame, true);
        content_view.setFrame(content_frame);
        new_layer.setFrame(content_frame);
        new_layer.setContentsScale(scale_factor);
        new_layer.setDrawableSize(drawable_size);

        let ptr: *mut std::ffi::c_void = Retained::as_ptr(&new_layer).cast_mut().cast();
        Ok(SendPtr(ptr))
    }

    unsafe fn install_web_view(
        self,
        html_path: &Path,
        read_access_root: &Path,
        initial_properties: Option<&wallpaper_web::Properties>,
    ) -> Result<SendPtr, wallpaper_web::WebError> {
        debug_assert!(NSThread::isMainThread_class());

        let web_view = unsafe {
            wallpaper_web::install_web_view(
                wallpaper_web::ObjcPtr::new(self.window.cast()),
                wallpaper_web::ObjcPtr::new(self.content_view.cast()),
                html_path,
                read_access_root,
                initial_properties,
            )
        }?;
        Ok(SendPtr(web_view.as_ptr()))
    }
}

pub struct MainThread {
    object: std::mem::ManuallyDrop<Retained<AnyObject>>,
}

impl MainThread {
    #[allow(clippy::unnecessary_wraps)]
    unsafe fn retain(object: &AnyObject, _label: &str) -> Result<Self, EngineError> {
        debug_assert!(objc2_foundation::NSThread::isMainThread_class());
        Ok(Self {
            object: std::mem::ManuallyDrop::new(object.retain()),
        })
    }

    #[allow(clippy::needless_pass_by_value, clippy::single_call_fn)]
    unsafe fn retain_from_ptr(ptr: SendPtr) -> Result<Self, EngineError> {
        debug_assert!(objc2_foundation::NSThread::isMainThread_class());
        if ptr.0.is_null() {
            return Err(EngineError::Platform(
                "MainThread::retain_from_ptr received null".to_string(),
            ));
        }

        unsafe {
            let object = &*(ptr.0.cast::<AnyObject>());
            Self::retain(object, "CAMetalLayer (replaced)")
        }
    }

    fn as_ptr(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.object).cast_mut()
    }

    unsafe fn release(mut self) {
        debug_assert!(objc2_foundation::NSThread::isMainThread_class());
        unsafe { std::mem::ManuallyDrop::drop(&mut self.object) };
    }

    pub fn dispatch<F, R>(body: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        if objc2_foundation::NSThread::isMainThread_class() {
            return body();
        }

        let mut context = MainThreadDispatchContext {
            body: Some(body),
            result: None,
        };

        // The stack context is valid for the duration of dispatch_sync_f because
        // the call blocks until the main queue has executed `invoke`.
        unsafe {
            dispatch2::DispatchQueue::main()
                .exec_sync_f((&raw mut context).cast(), invoke_main_thread_body::<F, R>);
        }

        match context
            .result
            .expect("main-thread body should complete before dispatch_sync_f returns")
        {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

#[allow(clippy::single_call_fn)]
pub(crate) fn run_on_main_thread<F, R>(body: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    MainThread::dispatch(body)
}

// SAFETY: This is an owned Objective-C pointer token, not a native facade. It
// may move between Rust threads, but retains, releases, and Objective-C message
// sends are only performed by the main-thread window path above.
unsafe impl Send for MainThread {}

#[cfg(test)]
mod tests {
    use super::{
        MouseButtonEdges, MouseButtonState, MouseButtonTracker, MouseButtons,
        NormalizedMousePosition,
    };

    #[test]
    fn mouse_buttons_reports_current_state_for_all_owe_buttons() {
        let states = MouseButtons::from_mask(0b101).states();

        assert_eq!(states.len(), 32);
        assert_eq!(states[0].button, 0);
        assert!(states[0].pressed);
        assert_eq!(states[1].button, 1);
        assert!(!states[1].pressed);
        assert_eq!(states[2].button, 2);
        assert!(states[2].pressed);
    }

    #[test]
    fn mouse_button_edges_preserve_press_and_release_in_one_poll() {
        let states = MouseButtonEdges::from_masks(0, 1, 1).states();

        assert_eq!(
            &states[..2],
            &[
                MouseButtonState {
                    button: 0,
                    pressed: true,
                },
                MouseButtonState {
                    button: 0,
                    pressed: false,
                },
            ]
        );
    }

    #[test]
    fn mouse_button_tracker_exposes_tap_edges_then_clears_them() {
        let mut tracker = MouseButtonTracker::new();

        tracker.set_button(0, true);
        tracker.set_button(0, false);

        let tap = tracker.consume_edges();
        assert_eq!(tap.down().mask(), 0);
        assert_eq!(
            &tap.states()[..2],
            &[
                MouseButtonState {
                    button: 0,
                    pressed: true,
                },
                MouseButtonState {
                    button: 0,
                    pressed: false,
                },
            ]
        );

        let idle = tracker.consume_edges();
        assert_eq!(idle.down().mask(), 0);
        assert_eq!(
            idle.states().iter().filter(|state| state.pressed).count(),
            0
        );
    }

    #[test]
    fn mouse_button_tracker_level_state_reports_only_real_edges() {
        let mut tracker = MouseButtonTracker::new();

        tracker.sync_down_mask(1);
        let press = tracker.consume_edges();
        assert_eq!(press.down().mask(), 1);
        assert_eq!(
            press
                .states()
                .iter()
                .filter(|state| state.button == 0 && state.pressed)
                .count(),
            1
        );

        tracker.sync_down_mask(1);
        let held = tracker.consume_edges();
        assert_eq!(held.down().mask(), 1);
        assert!(held.transitions().is_empty());

        tracker.sync_down_mask(0);
        let release = tracker.consume_edges();
        assert_eq!(release.down().mask(), 0);
        assert_eq!(
            release.transitions()[0],
            MouseButtonState {
                button: 0,
                pressed: false,
            }
        );
    }

    #[test]
    fn normalized_mouse_position_converts_appkit_points_to_owe_coordinates() {
        let position = NormalizedMousePosition::from_window_point(480.0, 270.0, 1920.0, 1080.0)
            .expect("valid point should normalize");

        assert_eq!(position, NormalizedMousePosition { x: 0.25, y: 0.75 });
    }

    #[test]
    fn normalized_mouse_position_clamps_and_rejects_invalid_geometry() {
        assert_eq!(
            NormalizedMousePosition::from_window_point(-10.0, 1200.0, 1920.0, 1080.0),
            Some(NormalizedMousePosition { x: 0.0, y: 0.0 })
        );
        assert_eq!(
            NormalizedMousePosition::from_window_point(f64::NAN, 0.0, 1920.0, 1080.0),
            None
        );
        assert_eq!(
            NormalizedMousePosition::from_window_point(0.0, 0.0, 0.0, 1080.0),
            None
        );
    }
}
