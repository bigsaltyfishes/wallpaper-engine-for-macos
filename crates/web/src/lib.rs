use std::{
    fs,
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
use serde_json::Value;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub enum WebError {
    InvalidInput(String),
    Platform(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Properties {
    values: serde_json::Map<String, Value>,
}

impl Properties {
    /// Loads Wallpaper Engine web user properties from a `project.json` file
    /// and applies a flat override object.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest or override JSON is invalid.
    pub fn load(project_json_path: &Path, override_json: Option<&str>) -> Result<Self, WebError> {
        let content = fs::read_to_string(project_json_path).map_err(|error| {
            WebError::InvalidInput(format!("failed to read web project properties: {error}"))
        })?;
        Self::parse(&content, override_json)
    }

    /// Parses Wallpaper Engine web user properties from project JSON text.
    ///
    /// # Errors
    ///
    /// Returns an error if the project or override JSON is invalid.
    pub fn parse(project_json: &str, override_json: Option<&str>) -> Result<Self, WebError> {
        let project: Value = serde_json::from_str(project_json).map_err(|error| {
            WebError::InvalidInput(format!("failed to parse web project properties: {error}"))
        })?;
        let properties = project
            .get("general")
            .and_then(Value::as_object)
            .and_then(|general| general.get("properties"))
            .and_then(Value::as_object);

        let overrides = override_json
            .map(|json| {
                serde_json::from_str::<Value>(json)
                    .map_err(|error| {
                        WebError::InvalidInput(format!(
                            "failed to parse web property override: {error}"
                        ))
                    })
                    .and_then(|value| {
                        value.as_object().cloned().ok_or_else(|| {
                            WebError::InvalidInput(
                                "web property override must be an object".to_string(),
                            )
                        })
                    })
            })
            .transpose()?
            .unwrap_or_default();

        let mut values = serde_json::Map::new();
        if let Some(properties) = properties {
            for (id, property) in properties {
                let Some(property) = property.as_object() else {
                    continue;
                };
                let mut entry = serde_json::Map::new();
                let value = overrides
                    .get(id)
                    .cloned()
                    .or_else(|| property.get("value").cloned())
                    .unwrap_or(Value::Null);
                entry.insert("value".to_string(), normalize_property_value(value));
                if property
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("combo"))
                {
                    if let Some(text) = combo_option_text(property, entry.get("value").unwrap()) {
                        entry.insert("text".to_string(), Value::String(text));
                    }
                }
                values.insert(id.clone(), Value::Object(entry));
            }

            for (id, value) in overrides {
                if !values.contains_key(&id) {
                    let mut entry = serde_json::Map::new();
                    entry.insert("value".to_string(), normalize_property_value(value));
                    values.insert(id, Value::Object(entry));
                }
            }
        }

        Ok(Self { values })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn to_json_string(&self) -> Result<String, WebError> {
        serde_json::to_string(&Value::Object(self.values.clone()))
            .map_err(|error| WebError::Platform(error.to_string()))
    }
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

fn normalize_property_value(value: Value) -> Value {
    match value {
        Value::Array(_) | Value::Object(_) => Value::String(value.to_string()),
        value => value,
    }
}

fn combo_option_text(
    property: &serde_json::Map<String, Value>,
    selected_value: &Value,
) -> Option<String> {
    let selected_value = scalar_to_string(selected_value);
    property
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_object)
        .find(|option| {
            option
                .get("value")
                .is_some_and(|value| scalar_to_string(value) == selected_value)
        })
        .and_then(|option| option.get("label").and_then(Value::as_str))
        .map(str::to_string)
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

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
    initial_properties: Option<&Properties>,
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
    unsafe { install_wallpaper_engine_user_script(&config, initial_properties) }?;

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

unsafe fn install_wallpaper_engine_user_script(
    config: &AnyObject,
    initial_properties: Option<&Properties>,
) -> Result<(), WebError> {
    let controller_class = AnyClass::get(c"WKUserContentController").ok_or_else(|| {
        WebError::Platform("WebKit WKUserContentController class is unavailable".to_string())
    })?;
    let script_class = AnyClass::get(c"WKUserScript").ok_or_else(|| {
        WebError::Platform("WebKit WKUserScript class is unavailable".to_string())
    })?;

    let initial_properties = initial_properties
        .map(Properties::to_json_string)
        .transpose()?
        .unwrap_or_else(|| "{}".to_string());
    let source = r#"
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
  let propertyListener;
  let pendingProperties = __INITIAL_PROPERTIES__;
  function applyProperties(properties) {
    if (!properties || typeof properties !== "object") return;
    if (propertyListener && typeof propertyListener.applyUserProperties === "function") {
      try { propertyListener.applyUserProperties(properties); } catch (_) {}
    } else {
      Object.assign(pendingProperties, properties);
    }
  }
  Object.defineProperty(window, "wallpaperPropertyListener", {
    configurable: true,
    enumerable: true,
    get: function() { return propertyListener; },
    set: function(listener) {
      propertyListener = listener;
      if (propertyListener && typeof propertyListener.applyUserProperties === "function" && Object.keys(pendingProperties).length > 0) {
        const properties = pendingProperties;
        pendingProperties = {};
        try { propertyListener.applyUserProperties(properties); } catch (_) {}
      }
    }
  });
  window.__wallpaperDispatchProperties = applyProperties;

  let currentScalingMode = "stretch";
  let currentScalingFactor = 1.0;

  function applyScaling() {
    const html = document.documentElement;
    html.style.transformOrigin = "0 0";
    if (currentScalingMode === "stretch" && Math.abs(currentScalingFactor - 1.0) < 0.001) {
      html.style.transform = "";
      return;
    }
    if (currentScalingMode === "none" || currentScalingMode === "stretch") {
      html.style.transform = "scale(" + currentScalingFactor + ")";
      return;
    }
    const body = document.body;
    const cw = body.scrollWidth || window.innerWidth;
    const ch = body.scrollHeight || window.innerHeight;
    if (cw <= 0 || ch <= 0) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const contentRatio = cw / ch;
    const viewRatio = vw / vh;
    let s;
    if (currentScalingMode === "fit") {
      s = contentRatio > viewRatio ? vw / cw : vh / ch;
    } else {
      s = contentRatio > viewRatio ? vh / ch : vw / cw;
    }
    s *= currentScalingFactor;
    const ox = (vw - cw * s) / 2;
    const oy = (vh - ch * s) / 2;
    html.style.transform = "translate(" + ox + "px, " + oy + "px) scale(" + s + ")";
  }

  window.__wallpaperSetScalingMode = function(mode) {
    currentScalingMode = mode;
    applyScaling();
  };
  window.__wallpaperSetScalingFactor = function(factor) {
    currentScalingFactor = factor;
    applyScaling();
  };

  let targetFps = 0;
  const origRAF = window.requestAnimationFrame;
  const origCAF = window.cancelAnimationFrame;
  let rafPending = [];
  let rafRunning = false;
  let rafLastTime = 0;

  function rafTick(now) {
    const elapsed = now - rafLastTime;
    const interval = targetFps > 0 ? 1000 / targetFps : 0;
    if (interval <= 0 || elapsed >= interval) {
      rafLastTime = now;
      const batch = rafPending;
      rafPending = [];
      for (let i = 0; i < batch.length; i++) {
        try { batch[i](now); } catch (_) {}
      }
    }
    if (rafPending.length > 0) {
      origRAF(rafTick);
    } else {
      rafRunning = false;
    }
  }

  window.requestAnimationFrame = function(cb) {
    rafPending.push(cb);
    if (!rafRunning) {
      rafRunning = true;
      rafLastTime = performance.now();
      origRAF(rafTick);
    }
    return 0;
  };

  window.cancelAnimationFrame = function() {
    rafPending = [];
    rafRunning = false;
  };

  window.__wallpaperSetFps = function(fps) {
    targetFps = fps;
  };

  window.addEventListener("resize", applyScaling);
  if (document.readyState === "complete") { applyScaling(); }
  else { window.addEventListener("load", applyScaling); }
})();
"#
    .replace("__INITIAL_PROPERTIES__", &initial_properties);
    let source = NSString::from_str(&source);
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
    property_dispatcher: PropertyDispatcher,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn start<F>(
        mut next_audio_frame: F,
        audio_dispatcher: AudioDispatcher,
        property_dispatcher: PropertyDispatcher,
    ) -> Self
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
                        let _ = audio_dispatcher.dispatch_audio_frame(&bins);
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
            })
            .ok();
        Self {
            property_dispatcher,
            stop,
            worker,
        }
    }

    pub fn dispatch_properties(&self, properties: &Properties) -> Result<(), WebError> {
        self.property_dispatcher.dispatch_properties(properties)
    }

    pub fn set_scaling_mode(&self, mode: &str) -> Result<(), WebError> {
        self.property_dispatcher.evaluate_js(&format!(
            "window.__wallpaperSetScalingMode && window.__wallpaperSetScalingMode(\"{mode}\");"
        ))
    }

    pub fn set_scaling_factor(&self, factor: f64) -> Result<(), WebError> {
        self.property_dispatcher.evaluate_js(&format!(
            "window.__wallpaperSetScalingFactor && window.__wallpaperSetScalingFactor({factor});"
        ))
    }

    pub fn set_fps(&self, fps: u32) -> Result<(), WebError> {
        self.property_dispatcher.evaluate_js(&format!(
            "window.__wallpaperSetFps && window.__wallpaperSetFps({fps});"
        ))
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

pub struct PropertyDispatcher {
    content_view: Option<MainThreadObject>,
}

impl PropertyDispatcher {
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

    pub fn dispatch_properties(&self, properties: &Properties) -> Result<(), WebError> {
        if properties.is_empty() {
            return Ok(());
        }
        let json = properties.to_json_string()?;
        let script = format!(
            "window.__wallpaperDispatchProperties && window.__wallpaperDispatchProperties({json});"
        );
        self.evaluate_js(&script)
    }

    pub fn evaluate_js(&self, script: &str) -> Result<(), WebError> {
        let Some(content_view) = self.content_view.as_ref() else {
            return Err(WebError::Platform("web dispatcher is closed".to_string()));
        };
        let content_view = ObjcPtr::new(content_view.as_ptr().cast());
        let script = script.to_string();
        MainThread::dispatch(move || unsafe {
            evaluate_js_on_view(content_view, &script);
        });
        Ok(())
    }
}

impl Drop for PropertyDispatcher {
    fn drop(&mut self) {
        if let Some(content_view) = self.content_view.take() {
            MainThread::dispatch(move || unsafe {
                content_view.release();
            });
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

unsafe fn dispatch_properties_to_view(content_view: ObjcPtr, json: &str) {
    debug_assert!(NSThread::isMainThread_class());
    evaluate_js_on_view(
        content_view,
        &format!(
            "window.__wallpaperDispatchProperties && window.__wallpaperDispatchProperties({json});"
        ),
    );
}

unsafe fn evaluate_js_on_view(content_view: ObjcPtr, script: &str) {
    debug_assert!(NSThread::isMainThread_class());
    let source = NSString::from_str(script);
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

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT_JSON: &str = r#"{
        "type": "web",
        "general": { "properties": {
            "enabled": { "type": "bool", "value": false },
            "size": { "type": "slider", "value": 10 },
            "tint": { "type": "color", "value": "0.1 0.2 0.3" },
            "choice": {
                "type": "combo",
                "value": "a",
                "options": [
                    { "label": "A", "value": "a" },
                    { "label": "B", "value": "b" }
                ]
            }
        }}
    }"#;

    #[test]
    fn parses_default_properties_for_apply_user_properties() {
        let properties = Properties::parse(PROJECT_JSON, None).expect("properties should parse");
        let json: Value =
            serde_json::from_str(&properties.to_json_string().expect("json should serialize"))
                .expect("payload should be json");

        assert_eq!(json["enabled"]["value"], false);
        assert_eq!(json["size"]["value"], 10);
        assert_eq!(json["tint"]["value"], "0.1 0.2 0.3");
        assert_eq!(json["choice"]["value"], "a");
        assert_eq!(json["choice"]["text"], "A");
    }

    #[test]
    fn applies_flat_property_overrides() {
        let properties = Properties::parse(
            PROJECT_JSON,
            Some(r#"{"enabled":true,"choice":"b","unknown":"value"}"#),
        )
        .expect("properties should parse");
        let json: Value =
            serde_json::from_str(&properties.to_json_string().expect("json should serialize"))
                .expect("payload should be json");

        assert_eq!(json["enabled"]["value"], true);
        assert_eq!(json["choice"]["value"], "b");
        assert_eq!(json["choice"]["text"], "B");
        assert_eq!(json["unknown"]["value"], "value");
    }

    #[test]
    fn preserves_sample_wallpaper_property_value_shapes() {
        let properties = Properties::parse(
            r#"{
                "type": "web",
                "general": { "properties": {
                    "screenFile": { "type": "file" },
                    "phoneText": {
                        "type": "textinput",
                        "value": "[{\"time\":0,\"text\":\"凌晨啦!\"}]"
                    },
                    "disableRili": { "type": "bool", "value": false }
                }}
            }"#,
            Some(
                r#"{
                    "screenFile": "/Users/wjj/Downloads/image.jpeg",
                    "phoneText": "[{\"time\":6,\"text\":\"早上好!\"}]",
                    "disableRili": true
                }"#,
            ),
        )
        .expect("properties should parse");
        let json: Value =
            serde_json::from_str(&properties.to_json_string().expect("json should serialize"))
                .expect("payload should be json");

        assert_eq!(
            json["screenFile"]["value"],
            "/Users/wjj/Downloads/image.jpeg"
        );
        assert_eq!(
            json["phoneText"]["value"],
            r#"[{"time":6,"text":"早上好!"}]"#
        );
        assert_eq!(json["disableRili"]["value"], true);
    }
}
