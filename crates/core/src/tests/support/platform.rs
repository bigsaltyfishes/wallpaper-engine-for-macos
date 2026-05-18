//! Shared platform-test helpers used by display and Vulkan surface tests.
//!
//! These helpers check system-level preconditions that can't be simulated in
//! a unit test (primary display availability, main-thread access, opt-in
//! environment variable for window tests). They are free functions because
//! they operate on system state rather than any single data structure.

pub fn primary_display_or_skip() -> Option<crate::DisplayDesc> {
    match crate::DisplayDesc::primary() {
        Ok(display) => Some(display),
        Err(crate::EngineError::Platform(message))
            if message.contains("CGMainDisplayID returned 0")
                || message.contains("no active displays") =>
        {
            eprintln!("skipping display-dependent test: {message}");
            None
        }
        Err(error) => panic!("display should exist: {error}"),
    }
}

pub fn window_tests_enabled_or_skip(kind: &str) -> bool {
    if std::env::var_os("WE_RUN_MACOS_WINDOW_TESTS").is_none() {
        eprintln!(
            "skipping {kind} platform test: set WE_RUN_MACOS_WINDOW_TESTS=1 to run AppKit window \
             tests"
        );
        return false;
    }

    if !objc2_foundation::NSThread::isMainThread_class() {
        eprintln!("skipping {kind} platform test: Rust test harness is not on AppKit main thread");
        return false;
    }

    true
}
