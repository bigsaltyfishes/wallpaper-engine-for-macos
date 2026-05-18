//! Private Open Wallpaper Engine integration.
//!
//! This module is intentionally not a runtime layer. Runtime state lives in
//! `crate::WallpaperEngine`; OWE is used only as the statically linked renderer
//! backend behind bindgen-generated `extern "C-unwind"` calls.

pub mod backend;
pub mod sys;
pub mod unwind;
