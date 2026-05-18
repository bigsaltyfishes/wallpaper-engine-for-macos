//! Unwind handling for calls into Open Wallpaper Engine.
//!
//! Every OWE symbol used from Rust is generated as `extern "C-unwind"`. Safe
//! wrappers call through [`call_ffi`] so a Rust-observable unwind becomes
//! `EngineError::Crash` instead of escaping into callers as undefined behavior.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::{EngineError, owe::sys};

pub struct UnwindSafeFFI {
    operation: &'static str,
}

impl UnwindSafeFFI {
    #[must_use]
    pub fn new(operation: &'static str) -> Self {
        Self { operation }
    }

    /// Retrieves the last error message from OWE, if any.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the OWE backend is properly initialized
    /// before calling this function, as it may involve FFI calls that
    /// require a valid OWE context.
    #[must_use]
    pub unsafe fn last_error() -> String {
        unsafe {
            let this = Self {
                operation: "last_error",
            };
            match this.call(|| sys::owe_last_error()) {
                Ok(message) => {
                    if message.is_null() {
                        "open-wallpaper-engine failed".to_string()
                    } else {
                        let message = std::ffi::CStr::from_ptr(message)
                            .to_string_lossy()
                            .into_owned();
                        if message.is_empty() {
                            "open-wallpaper-engine failed".to_string()
                        } else {
                            message
                        }
                    }
                }
                Err(error) => error.to_string(),
            }
        }
    }

    /// Executes one FFI call and converts an unwind into
    /// [`EngineError::Crash`].
    ///
    /// # Safety
    ///
    /// The caller must ensure the closure performs only FFI operations whose
    /// ABI permits unwinding across the boundary and that all raw pointers
    /// passed to the closure are valid for the duration of the call.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Crash`] when the wrapped FFI call unwinds.
    pub unsafe fn call<T>(self, call: impl FnOnce() -> T) -> Result<T, EngineError> {
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => Ok(value),
            Err(payload) => {
                let payload = if let Some(message) = payload.downcast_ref::<&'static str>() {
                    (*message).to_string()
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    message.clone()
                } else {
                    "unknown unwind payload".to_string()
                };
                Err(EngineError::Crash(format!(
                    "open-wallpaper-engine unwound while calling {}: {}",
                    self.operation, payload
                )))
            }
        }
    }
}

pub mod testing {
    use super::UnwindSafeFFI;
    use crate::EngineError;

    /// Forces a panic through the C-unwind wrapper for tests.
    ///
    /// # Errors
    ///
    /// Always returns [`EngineError::Crash`] when unwind catching works.
    ///
    /// # Panics
    ///
    /// Panics if the platform/runtime fails to catch the test unwind.
    pub fn panic_across_c_unwind_for_testing() -> Result<i32, EngineError> {
        unsafe {
            UnwindSafeFFI::new("panic_for_testing").call(|| -> i32 { panic!("test ffi unwind") })
        }
    }
}
