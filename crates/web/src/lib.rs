use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use objc2::{
    Message, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_app_kit::{NSView, NSWindow};
use objc2_foundation::{NSString, NSThread, NSURL};

#[derive(Debug)]
pub enum WebError {
    InvalidInput(String),
    Platform(String),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) | Self::Platform(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for WebError {}

#[derive(Clone, Copy)]
pub struct ObjcPtr(*mut std::ffi::c_void);

impl ObjcPtr {
    #[must_use]
    pub fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self(ptr)
    }

    #[must_use]
    pub fn as_ptr(self) -> *mut std::ffi::c_void {
        self.0
    }
}

// SAFETY: The pointer value is only transported across Rust threads. All
// Objective-C dereferences and reference-count operations are performed on the
// main thread.
unsafe impl Send for ObjcPtr {}

/// Installs a `WKWebView` into an existing `NSWindow` and loads a local HTML
/// entry file.
///
/// # Safety
///
/// `window` must point to a live `NSWindow`, `current_content_view` must point
/// to its current `NSView`, and this function must run on the main thread.
pub unsafe fn install_web_view(
    window: ObjcPtr,
    current_content_view: ObjcPtr,
    html_path: &Path,
    read_access_root: &Path,
) -> Result<ObjcPtr, WebError> {
    debug_assert!(NSThread::isMainThread_class());

    let web_view_class = AnyClass::get(c"WKWebView")
        .ok_or_else(|| WebError::Platform("WebKit WKWebView class is unavailable".to_string()))?;
    let config_class = AnyClass::get(c"WKWebViewConfiguration").ok_or_else(|| {
        WebError::Platform("WebKit WKWebViewConfiguration class is unavailable".to_string())
    })?;

    let window = unsafe { &*(window.as_ptr().cast::<NSWindow>()) };
    let current_content_view = unsafe { &*(current_content_view.as_ptr().cast::<NSView>()) };
    let frame = current_content_view.frame();

    let config: *mut AnyObject = unsafe { msg_send![config_class, new] };
    let config = unsafe { Retained::from_raw(config) }.ok_or_else(|| {
        WebError::Platform("WKWebViewConfiguration allocation returned null".to_string())
    })?;
    unsafe { install_wallpaper_engine_user_script(&config) }?;

    let web_view: *mut AnyObject = unsafe { msg_send![web_view_class, alloc] };
    let web_view: *mut AnyObject =
        unsafe { msg_send![web_view, initWithFrame: frame, configuration: &*config] };
    let web_view = unsafe { Retained::from_raw(web_view) }
        .ok_or_else(|| WebError::Platform("WKWebView initialization returned null".to_string()))?;

    let html_path_string = html_path
        .to_str()
        .ok_or_else(|| WebError::InvalidInput("html_path is not valid UTF-8".to_string()))?;
    let read_access_root_string = read_access_root
        .to_str()
        .ok_or_else(|| WebError::InvalidInput("read_access_root is not valid UTF-8".to_string()))?;
    let html = NSString::from_str(html_path_string);
    let root = NSString::from_str(read_access_root_string);
    let html_url = NSURL::fileURLWithPath(&html);
    let root_url = NSURL::fileURLWithPath_isDirectory(&root, true);
    let _: *mut AnyObject = unsafe {
        msg_send![&*web_view, loadFileURL: &*html_url, allowingReadAccessToURL: &*root_url]
    };

    let web_view_as_view = unsafe { &*(Retained::as_ptr(&web_view).cast::<NSView>()) };
    web_view_as_view.setFrame(frame);
    window.setContentView(Some(web_view_as_view));

    Ok(ObjcPtr::new(Retained::as_ptr(&web_view).cast_mut().cast()))
}

unsafe fn install_wallpaper_engine_user_script(config: &AnyObject) -> Result<(), WebError> {
    let controller_class = AnyClass::get(c"WKUserContentController").ok_or_else(|| {
        WebError::Platform("WebKit WKUserContentController class is unavailable".to_string())
    })?;
    let script_class = AnyClass::get(c"WKUserScript").ok_or_else(|| {
        WebError::Platform("WebKit WKUserScript class is unavailable".to_string())
    })?;

    let source = NSString::from_str(
        r#"
(() => {
  const listeners = [];
  window.wallpaperRegisterAudioListener = function(listener) {
    if (typeof listener === "function") {
      listeners.push(listener);
      listener(new Array(128).fill(0));
    }
  };
  window.__wallpaperDispatchAudio = function(data) {
    if (!Array.isArray(data) || data.length < 128) return;
    const frame = data.slice(0, 128);
    for (const listener of listeners.slice()) {
      try { listener(frame); } catch (_) {}
    }
  };
})();
"#,
    );
    let controller: *mut AnyObject = unsafe { msg_send![controller_class, new] };
    let controller = unsafe { Retained::from_raw(controller) }.ok_or_else(|| {
        WebError::Platform("WKUserContentController allocation returned null".to_string())
    })?;
    let script: *mut AnyObject = unsafe { msg_send![script_class, alloc] };
    let script: *mut AnyObject = unsafe {
        msg_send![
            script,
            initWithSource: &*source,
            injectionTime: 0isize,
            forMainFrameOnly: false
        ]
    };
    let script = unsafe { Retained::from_raw(script) }.ok_or_else(|| {
        WebError::Platform("WKUserScript initialization returned null".to_string())
    })?;
    let _: () = unsafe { msg_send![&*controller, addUserScript: &*script] };
    let _: () = unsafe { msg_send![config, setUserContentController: &*controller] };
    Ok(())
}

pub struct AudioDispatcher {
    content_view: Option<MainThreadObject>,
}

impl AudioDispatcher {
    /// # Safety
    ///
    /// `content_view` must point to a live `WKWebView`/`NSView` object.
    pub unsafe fn retain(content_view: ObjcPtr) -> Result<Self, WebError> {
        let content_view = MainThread::dispatch(move || unsafe {
            MainThreadObject::retain_from_ptr(content_view)
        })?;
        Ok(Self {
            content_view: Some(content_view),
        })
    }

    pub fn dispatch_audio_frame(&self, bins: &[f32; 128]) -> Result<(), WebError> {
        let json = serde_json::to_string(&bins[..])
            .map_err(|error| WebError::Platform(error.to_string()))?;
        let Some(content_view) = self.content_view.as_ref() else {
            return Err(WebError::Platform(
                "web audio dispatcher is closed".to_string(),
            ));
        };
        let content_view = ObjcPtr::new(content_view.as_ptr().cast());
        MainThread::dispatch(move || unsafe {
            dispatch_audio_frame_to_view(content_view, &json);
        });
        Ok(())
    }
}

impl Drop for AudioDispatcher {
    fn drop(&mut self) {
        if let Some(content_view) = self.content_view.take() {
            MainThread::dispatch(move || unsafe {
                content_view.release();
            });
        }
    }
}

pub struct Runtime {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn start<F>(mut next_audio_frame: F, dispatcher: AudioDispatcher) -> Self
    where
        F: FnMut() -> Option<[f32; 128]> + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::Builder::new()
            .name("wallpaper-web-audio-dispatch".to_string())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    if let Some(bins) = next_audio_frame() {
                        let _ = dispatcher.dispatch_audio_frame(&bins);
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
            })
            .ok();
        Self { stop, worker }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

unsafe fn dispatch_audio_frame_to_view(content_view: ObjcPtr, json: &str) {
    debug_assert!(NSThread::isMainThread_class());
    let source = NSString::from_str(&format!(
        "window.__wallpaperDispatchAudio && window.__wallpaperDispatchAudio({json});"
    ));
    let web_view = unsafe { &*(content_view.as_ptr().cast::<AnyObject>()) };
    let _: () = unsafe {
        msg_send![web_view, evaluateJavaScript: &*source, completionHandler: std::ptr::null::<AnyObject>()]
    };
}

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

struct MainThread;

impl MainThread {
    fn dispatch<F, R>(body: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        if NSThread::isMainThread_class() {
            return body();
        }

        let mut context = MainThreadDispatchContext {
            body: Some(body),
            result: None,
        };

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

struct MainThreadObject {
    object: std::mem::ManuallyDrop<Retained<AnyObject>>,
}

impl MainThreadObject {
    unsafe fn retain_from_ptr(ptr: ObjcPtr) -> Result<Self, WebError> {
        debug_assert!(NSThread::isMainThread_class());
        if ptr.as_ptr().is_null() {
            return Err(WebError::Platform(
                "MainThreadObject::retain_from_ptr received null".to_string(),
            ));
        }

        let object = unsafe { &*(ptr.as_ptr().cast::<AnyObject>()) };
        Ok(Self {
            object: std::mem::ManuallyDrop::new(object.retain()),
        })
    }

    fn as_ptr(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.object).cast_mut()
    }

    unsafe fn release(mut self) {
        debug_assert!(NSThread::isMainThread_class());
        unsafe { std::mem::ManuallyDrop::drop(&mut self.object) };
    }
}

// SAFETY: This owns an Objective-C retain but all reference-count operations
// and message sends are dispatched to the main thread.
unsafe impl Send for MainThreadObject {}
