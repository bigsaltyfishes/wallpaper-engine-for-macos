//! Safe Rust ownership wrapper for Open Wallpaper Engine renderer objects.
//!
//! The generated `sys` module is the only place Rust sees OWE's C symbols.
//! Everything above it speaks in wallpaper-core domain types (`SceneDesc`,
//! `ScalingMode`, audio frames) and owns only an opaque renderer pointer. There
//! are deliberately no raw scene/display descriptor structs here;
//! reconciliation and runtime state stay in `crate::engine`.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::Arc,
};

use serde_json::Value;

use crate::{
    EngineError,
    media::audio::{AudioVolume, InterleavedStereoF32, MonoPcmF32},
    owe::{sys, unwind::UnwindSafeFFI},
    project::{ScalingMode, SceneDesc, SerdeValudeExt},
};

/// Private handle to the statically linked Open Wallpaper Engine renderer.
#[derive(Clone, Copy, Debug, Default)]
pub struct OweBackend;

pub type FirstFrameCallback = Arc<dyn Fn() + Send + Sync + 'static>;

impl OweBackend {
    /// Initializes access to the statically linked backend.
    ///
    /// OWE currently has no global renderer initialization entry point that is
    /// separate from `SceneWallpaper::init`, so this is intentionally a no-op.
    ///
    /// # Errors
    ///
    /// Reserved for future backend initialization failures; currently always
    /// returns `Ok`.
    pub fn initialize() -> Result<Self, EngineError> {
        shader::ffi::ensure_linked();
        unsafe {
            UnwindSafeFFI::new("owe_set_log_callback")
                .call(|| sys::owe_set_log_callback(Some(owe_log_callback)))?;
        }
        Ok(Self)
    }

    /// Creates and fully configures one OWE renderer scene.
    ///
    /// `metal_layer` must be the `CAMetalLayer` owned by the paired
    /// `WallpaperWindow`. Render resolution is constructor-only because OWE
    /// bakes it into the Vulkan surface setup.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the OWE scene cannot be allocated or any
    /// renderer initialization/configuration call fails.
    pub fn open_scene(
        &self,
        desc: &SceneDesc,
        metal_layer: *mut c_void,
        scaling_mode: ScalingMode,
        scaling_factor: f64,
        render_resolution: Option<(u32, u32)>,
        first_frame_callback: Option<FirstFrameCallback>,
    ) -> Result<OweScene, EngineError> {
        let mut raw = std::ptr::null_mut();
        call_status("owe_scene_wallpaper_new", || unsafe {
            sys::owe_scene_wallpaper_new(&raw mut raw)
        })?;

        let raw = NonNull::new(raw).ok_or_else(|| {
            EngineError::Render("open-wallpaper-engine returned a null scene".to_string())
        })?;
        let mut scene = OweScene {
            raw: Some(raw),
            render_initialized: false,
        };
        scene.initialize_renderer(desc, metal_layer, render_resolution)?;
        scene.apply_scene_config(desc)?;
        scene.set_first_frame_callback(first_frame_callback)?;
        scene.set_scaling_mode(scaling_mode)?;
        scene.set_scaling_factor(scaling_factor)?;
        scene.set_horizontal_flip(desc.horizontal_flip)?;
        Ok(scene)
    }

    /// Submits interleaved stereo PCM to OWE's audio response input.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] when OWE rejects the audio frame upload.
    pub fn submit_audio_frames(&self, frames: InterleavedStereoF32<'_>) -> Result<(), EngineError> {
        call_status("owe_audio_submit_frames", || unsafe {
            sys::owe_audio_submit_frames(
                frames.sample_rate(),
                frames.frame_count(),
                frames.samples().as_ptr(),
            )
        })
    }

    /// Submits mono PCM to OWE's audio response input.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] when OWE rejects the audio frame upload.
    pub fn submit_audio_mono_frames(&self, frames: &MonoPcmF32<'_>) -> Result<(), EngineError> {
        call_status("owe_audio_submit_mono_frames", || unsafe {
            sys::owe_audio_submit_mono_frames(
                frames.sample_rate(),
                frames.frame_count(),
                frames.samples().as_ptr(),
            )
        })
    }

    /// Returns the latest 128-bin Wallpaper Engine audio spectrum.
    ///
    /// The first 64 bins are left/mono, and the second 64 bins are right/mono.
    /// When no system audio has been captured, OWE returns zeros.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] when OWE rejects the output buffer.
    pub fn current_audio_spectrum_128(&self) -> Result<([f32; 128], u64), EngineError> {
        let mut bins = [0.0; 128];
        let mut generation = 0;
        call_status("owe_audio_current_spectrum_128", || unsafe {
            sys::owe_audio_current_spectrum_128(bins.as_mut_ptr(), bins.len(), &raw mut generation)
        })?;
        Ok((bins, generation))
    }
}

/// Owned `wallpaper::SceneWallpaper` backend object.
pub struct OweScene {
    raw: Option<NonNull<sys::owe_scene_wallpaper>>,
    render_initialized: bool,
}

// SAFETY: The pointer is an owned renderer token. Rust mutates it behind the
// engine mutex, and destruction is routed through the OWE delete wrapper.
unsafe impl Send for OweScene {}

impl OweScene {
    /// Retrieves the last error message from OWE, if any.
    fn property_name(
        operation: &'static str,
        call: impl FnOnce() -> *const c_char,
    ) -> Result<*const c_char, EngineError> {
        // Property names are owned by OWE constants. Keeping lookups behind
        // functions avoids duplicating string literals that must match upstream.
        let name = unsafe { UnwindSafeFFI::new(operation).call(call) }?;
        if name.is_null() {
            Err(EngineError::Render(
                "open-wallpaper-engine returned a null property name".to_string(),
            ))
        } else {
            Ok(name)
        }
    }

    /// Retrieves the current value of the scaling mode property from OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn scaling_mode() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_scaling_mode", || unsafe {
            sys::owe_property_scaling_mode()
        })
    }

    /// Retrieves the current value of the scaling factor property from OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn scaling_factor() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_scaling_factor", || unsafe {
            sys::owe_property_scaling_factor()
        })
    }

    /// Retrieves the current value of the horizontal flip property from OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn horizontal_flip() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_horizontal_flip", || unsafe {
            sys::owe_property_horizontal_flip()
        })
    }

    /// Retrieves the current value of the audio response enabled property from
    /// OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn audio_response_enabled() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_audio_response_enabled", || unsafe {
            sys::owe_property_audio_response_enabled()
        })
    }

    /// Retrieves the current value of the media integration enabled property
    /// from OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn media_integration_enabled() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_media_integration_enabled", || unsafe {
            sys::owe_property_media_integration_enabled()
        })
    }

    /// Retrieves the current value of the force shader refresh property from
    /// OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn force_shader_refresh() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_force_shader_refresh", || unsafe {
            sys::owe_property_force_shader_refresh()
        })
    }

    /// Retrieves the current value of the project property override JSON from
    /// OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn project_property_override_json() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_project_property_override_json", || unsafe {
            sys::owe_property_project_property_override_json()
        })
    }

    /// Retrieves the current value of the project property reset from OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if OWE returns a null property name or
    /// [`EngineError::Crash`] if the FFI call unwinds.
    pub fn project_property_reset() -> Result<*const c_char, EngineError> {
        Self::property_name("owe_property_project_property_reset", || unsafe {
            sys::owe_property_project_property_reset()
        })
    }
}

impl OweScene {
    /// Applies the descriptor-driven scene configuration to an existing
    /// renderer.
    ///
    /// This covers project source/assets paths, shader cache path, target FPS,
    /// pause state, audio-response defaults, force-refresh state, and
    /// descriptor property overrides. Callers that maintain mutable runtime
    /// overrides must reapply them after this method.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when descriptor paths contain interior NUL
    /// bytes, project override JSON cannot be flattened, shader cache
    /// preparation fails, or OWE rejects any property/configuration update.
    pub fn apply_scene_config(&mut self, desc: &SceneDesc) -> Result<(), EngineError> {
        let source = cstring(&desc.scene_path)?;
        let assets = cstring(&desc.assets_path)?;
        let cache_path = desc
            .shader_cache_path()?
            .as_deref()
            .map(cstring)
            .transpose()?;
        let property_override_json = desc
            .property_override_json
            .as_deref()
            .map(|json| {
                let flat_json = serde_json::from_str::<Value>(json)
                    .map_err(|e| EngineError::InvalidInput(e.to_string()))?
                    .flatten()?;
                serde_json::to_string(&flat_json)
                    .map_err(|e| EngineError::InvalidInput(e.to_string()))
            })
            .transpose()?;
        let property_override_json = property_override_json.as_deref().map(cstring).transpose()?;

        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_apply_config", || unsafe {
            sys::owe_scene_wallpaper_apply_config(
                raw.as_ptr(),
                source.as_ptr(),
                assets.as_ptr(),
                cache_path
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                desc.fps,
                desc.paused,
                desc.force_shader_refresh,
                property_override_json
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
            )
        })?;

        self.set_audio_response_enabled(desc.audio_response_enabled)?;
        self.set_audio_volume(desc.audio_volume)?;
        self.set_audio_muted(desc.audio_muted)?;
        Ok(())
    }

    /// Pauses rendering and releases the Vulkan surface + swapchain on the
    /// backend. Scene, shaders, textures, audio, and runtime state remain
    /// loaded. Synchronous — blocks until the render thread confirms the
    /// transaction's begin step.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidInput`] if the scene is already closed, or
    /// [`EngineError::Render`] if OWE fails the reconfigure-begin transaction.
    pub fn begin_surface_reconfigure(&mut self) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_begin_surface_reconfigure", || unsafe {
            sys::owe_scene_wallpaper_begin_surface_reconfigure(raw.as_ptr())
        })
    }

    /// Rebuilds the Vulkan surface + swapchain from a new Metal layer,
    /// rebuilds the render graph, and resumes rendering. Dimensions replace
    /// those used during the last init or finish. Synchronous — blocks until
    /// the render thread confirms the new surface is presentable.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidInput`] if `metal_layer` is null or the
    /// scene is closed, or [`EngineError::Render`] if OWE fails the finish
    /// step.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn finish_surface_reconfigure(
        &mut self,
        metal_layer: *mut c_void,
        width: u32,
        height: u32,
        render_width: u32,
        render_height: u32,
        scale_factor: f64,
    ) -> Result<(), EngineError> {
        if metal_layer.is_null() {
            return Err(EngineError::InvalidInput(
                "metal_layer must not be null".to_string(),
            ));
        }
        let raw = self.raw_ptr()?;
        call_status(
            "owe_scene_wallpaper_finish_surface_reconfigure",
            || unsafe {
                sys::owe_scene_wallpaper_finish_surface_reconfigure(
                    raw.as_ptr(),
                    metal_layer,
                    width,
                    height,
                    render_width,
                    render_height,
                    scale_factor,
                )
            },
        )
    }

    /// Sets the renderer scaling mode property.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the property update.
    pub fn set_scaling_mode(&mut self, mode: ScalingMode) -> Result<(), EngineError> {
        let value = match mode {
            ScalingMode::None => 0,
            ScalingMode::Stretch => 1,
            ScalingMode::Fit => 2,
            ScalingMode::Fill => 3,
        };
        self.set_property_int32(Self::scaling_mode()?, value)
    }

    /// Sets the renderer scaling factor property.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the property update.
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_scaling_factor(&mut self, factor: f64) -> Result<(), EngineError> {
        self.set_property_float(Self::scaling_factor()?, factor as f32)
    }

    /// Sets whether the renderer should mirror final presentation left-to-right.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the property update.
    pub fn set_horizontal_flip(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.set_property_bool(Self::horizontal_flip()?, enabled)
    }

    /// Sets the target renderer frame rate.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// update.
    pub fn set_target_fps(&mut self, fps: u32) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_target_fps", || unsafe {
            sys::owe_scene_wallpaper_set_target_fps(raw.as_ptr(), fps)
        })
    }

    /// Sets the renderer pause state.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// update.
    pub fn set_paused(&mut self, paused: bool) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_paused", || unsafe {
            sys::owe_scene_wallpaper_set_paused(raw.as_ptr(), paused)
        })
    }

    /// Registers a first-frame callback owned by this renderer scene.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// callback registration.
    pub fn set_first_frame_callback(
        &mut self,
        callback: Option<FirstFrameCallback>,
    ) -> Result<(), EngineError> {
        let Some(callback) = callback else {
            let raw = self.raw_ptr()?;
            return call_status("owe_scene_wallpaper_set_first_frame_callback", || unsafe {
                sys::owe_scene_wallpaper_set_first_frame_callback(
                    raw.as_ptr(),
                    Some(owe_first_frame_callback),
                    std::ptr::null_mut(),
                    Some(owe_first_frame_callback_drop),
                )
            });
        };

        let user_data = Box::into_raw(Box::new(callback)).cast::<c_void>();
        let raw = self.raw_ptr()?;
        let result = call_status("owe_scene_wallpaper_set_first_frame_callback", || unsafe {
            sys::owe_scene_wallpaper_set_first_frame_callback(
                raw.as_ptr(),
                Some(owe_first_frame_callback),
                user_data,
                Some(owe_first_frame_callback_drop),
            )
        });
        if result.is_err() {
            // SAFETY: `user_data` was produced by `Box::into_raw` immediately
            // above and OWE rejected the registration, so Rust must reclaim
            // that box.
            drop(unsafe { Box::<FirstFrameCallback>::from_raw(user_data.cast()) });
        }
        result
    }

    /// Sends normalized mouse coordinates to the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if coordinates are non-finite, the scene is
    /// closed, or OWE rejects the update.
    pub fn set_mouse_position(&mut self, x: f64, y: f64) -> Result<(), EngineError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(EngineError::InvalidInput(
                "mouse coordinates must be finite".to_string(),
            ));
        }
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_mouse_input", || unsafe {
            sys::owe_scene_wallpaper_mouse_input(raw.as_ptr(), x, y)
        })
    }

    /// Sends one mouse button transition to the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the button is out of range, the scene is
    /// closed, or OWE rejects the update.
    pub fn set_mouse_button(&mut self, button: u32, pressed: bool) -> Result<(), EngineError> {
        let button = i32::try_from(button).map_err(|_| {
            EngineError::InvalidInput("mouse button must be in range 0..31".to_string())
        })?;
        if !(0..=31).contains(&button) {
            return Err(EngineError::InvalidInput(
                "mouse button must be in range 0..31".to_string(),
            ));
        }
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_mouse_button", || unsafe {
            sys::owe_scene_wallpaper_mouse_button(raw.as_ptr(), button, pressed)
        })
    }

    /// Sends mouse enter/leave state to the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// update.
    pub fn set_mouse_entered(&mut self, entered: bool) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_mouse_enter", || unsafe {
            sys::owe_scene_wallpaper_mouse_enter(raw.as_ptr(), entered)
        })
    }

    /// Enables or disables audio-response behavior.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the property update.
    pub fn set_audio_response_enabled(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.set_property_bool(Self::audio_response_enabled()?, enabled)
    }

    /// Sets the scene-wide audio volume multiplier.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// update.
    pub fn set_audio_volume(&mut self, volume: AudioVolume) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_audio_volume", || unsafe {
            sys::owe_scene_wallpaper_set_audio_volume(raw.as_ptr(), volume.into())
        })
    }

    /// Sets the scene-wide audio mute flag.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// update.
    pub fn set_audio_muted(&mut self, muted: bool) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_audio_muted", || unsafe {
            sys::owe_scene_wallpaper_set_audio_muted(raw.as_ptr(), muted)
        })
    }

    /// Enables or disables media integration behavior.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the property update.
    #[allow(dead_code)]
    pub fn set_media_integration_enabled(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.set_property_bool(Self::media_integration_enabled()?, enabled)
    }

    /// Submits one serialized media integration event to OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if event serialization fails, the JSON contains
    /// an interior NUL byte, the scene is closed, or OWE rejects the event.
    #[allow(dead_code)]
    pub fn submit_media_event(
        &mut self,
        event: &crate::media::MediaIntegrationEvent,
    ) -> Result<(), EngineError> {
        let json = event
            .to_json()
            .map_err(|error| EngineError::InvalidInput(error.to_string()))?;
        let json = cstring(&json)?;
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_submit_media_event_json", || unsafe {
            sys::owe_scene_wallpaper_submit_media_event_json(raw.as_ptr(), json.as_ptr())
        })
    }

    /// Uploads system media artwork to OWE.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed or OWE rejects the
    /// artwork.
    #[allow(dead_code)]
    pub fn apply_system_media_artwork(
        &mut self,
        artwork: &crate::media::MediaThumbnailRgba,
    ) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status(
            "owe_scene_wallpaper_apply_system_media_artwork",
            || unsafe {
                sys::owe_scene_wallpaper_apply_system_media_artwork(
                    raw.as_ptr(),
                    artwork.width,
                    artwork.height,
                    artwork.rgba.as_ptr(),
                    artwork.rgba.len(),
                )
            },
        )
    }

    /// Applies a flattened project property override JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the JSON contains an interior NUL byte, the
    /// scene is closed, OWE property lookup fails, or OWE rejects the update.
    pub fn set_property_override(&mut self, flat_json: &str) -> Result<(), EngineError> {
        let encoded = cstring(flat_json)?;
        self.set_force_shader_refresh(true)?;
        self.set_property_string(Self::project_property_override_json()?, encoded.as_ptr())
    }

    /// Clears any project property override from the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the scene is closed, OWE property lookup
    /// fails, or OWE rejects the update.
    pub fn reset_property_override(&mut self) -> Result<(), EngineError> {
        self.set_force_shader_refresh(true)?;
        self.set_property_bool(Self::project_property_reset()?, true)
    }

    /// Closes and deletes the OWE renderer object.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Render`] if any OWE quiesce/shutdown/delete call
    /// fails. The scene pointer is still consumed so repeated close is
    /// harmless.
    pub fn close(&mut self) -> Result<(), EngineError> {
        let Some(raw) = self.raw.take() else {
            return Ok(());
        };

        let quiesce = if self.render_initialized {
            call_status("owe_scene_wallpaper_begin_surface_reconfigure", || unsafe {
                sys::owe_scene_wallpaper_begin_surface_reconfigure(raw.as_ptr())
            })
        } else {
            Ok(())
        };
        let shutdown = call_status("owe_scene_wallpaper_shutdown", || unsafe {
            sys::owe_scene_wallpaper_shutdown(raw.as_ptr())
        });
        let delete = call_status("owe_scene_wallpaper_delete", || unsafe {
            sys::owe_scene_wallpaper_delete(raw.as_ptr())
        });
        quiesce.and(shutdown).and(delete)
    }

    fn initialize_renderer(
        &mut self,
        desc: &SceneDesc,
        metal_layer: *mut c_void,
        render_resolution: Option<(u32, u32)>,
    ) -> Result<(), EngineError> {
        // `RenderInitInfo` contains C++ containers and callbacks, so the OWE
        // wrapper builds it on the C++ side from these primitive arguments.
        let (render_width, render_height) = render_resolution.unwrap_or((0, 0));
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_init", || unsafe {
            sys::owe_scene_wallpaper_init(raw.as_ptr())
        })?;
        call_status("owe_scene_wallpaper_init_metal_vulkan", || unsafe {
            sys::owe_scene_wallpaper_init_metal_vulkan(
                raw.as_ptr(),
                metal_layer,
                desc.display.width,
                desc.display.height,
                render_width,
                render_height,
                desc.display.scale_factor,
            )
        })?;
        self.render_initialized = true;
        Ok(())
    }

    fn set_force_shader_refresh(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.set_property_bool(Self::force_shader_refresh()?, enabled)
    }

    fn set_property_bool(&mut self, name: *const c_char, value: bool) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_property_bool", || unsafe {
            sys::owe_scene_wallpaper_set_property_bool(raw.as_ptr(), name, value)
        })
    }

    fn set_property_int32(&mut self, name: *const c_char, value: i32) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_property_int32", || unsafe {
            sys::owe_scene_wallpaper_set_property_int32(raw.as_ptr(), name, value)
        })
    }

    fn set_property_float(&mut self, name: *const c_char, value: f32) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_property_float", || unsafe {
            sys::owe_scene_wallpaper_set_property_float(raw.as_ptr(), name, value)
        })
    }

    fn set_property_string(
        &mut self,
        name: *const c_char,
        value: *const c_char,
    ) -> Result<(), EngineError> {
        let raw = self.raw_ptr()?;
        call_status("owe_scene_wallpaper_set_property_string", || unsafe {
            sys::owe_scene_wallpaper_set_property_string(raw.as_ptr(), name, value)
        })
    }

    fn raw_ptr(&self) -> Result<NonNull<sys::owe_scene_wallpaper>, EngineError> {
        self.raw
            .ok_or_else(|| EngineError::InvalidInput("scene is already closed".to_string()))
    }
}

impl Drop for OweScene {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

// TODO: Remove this
fn cstring(value: &str) -> Result<CString, EngineError> {
    CString::new(value).map_err(|error| EngineError::InvalidInput(error.to_string()))
}

fn call_status(operation: &'static str, call: impl FnOnce() -> c_int) -> Result<(), EngineError> {
    let status = unsafe { UnwindSafeFFI::new(operation).call(call) }?;
    if status == 0 {
        Ok(())
    } else {
        Err(EngineError::Render(unsafe { UnwindSafeFFI::last_error() }))
    }
}

#[allow(clippy::single_call_fn)]
unsafe extern "C-unwind" fn owe_log_callback(
    level: c_int,
    file: *const c_char,
    line: c_int,
    message: *const c_char,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let file = copy_c_string(file).unwrap_or_else(|| "unknown".to_string());
        let message = copy_c_string(message).unwrap_or_default();
        let level = match level {
            1 => log::Level::Error,
            _ => log::Level::Info,
        };
        let line = u32::try_from(line).ok();

        let args = format_args!("{message}");
        let record = log::Record::builder()
            .args(args)
            .level(level)
            .target("open_wallpaper_engine")
            .file(Some(&file))
            .line(line)
            .build();
        log::logger().log(&record);
    }));
}

#[allow(clippy::single_call_fn)]
unsafe extern "C-unwind" fn owe_first_frame_callback(user_data: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if user_data.is_null() {
            return;
        }

        // SAFETY: `user_data` is produced by `OweScene::set_first_frame_callback`
        // from `Box<FirstFrameCallback>`. The C++ wrapper owns that box until
        // it invokes `owe_first_frame_callback_drop`.
        let callback = unsafe { &*user_data.cast::<FirstFrameCallback>() };
        callback();
    }));
}

#[allow(clippy::single_call_fn)]
unsafe extern "C-unwind" fn owe_first_frame_callback_drop(user_data: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if user_data.is_null() {
            return;
        }

        // SAFETY: C++ calls this exactly once for each `Box<FirstFrameCallback>`
        // transferred through `owe_scene_wallpaper_set_first_frame_callback`.
        drop(unsafe { Box::<FirstFrameCallback>::from_raw(user_data.cast()) });
    }));
}

fn copy_c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }

    Some(
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned(),
    )
}
