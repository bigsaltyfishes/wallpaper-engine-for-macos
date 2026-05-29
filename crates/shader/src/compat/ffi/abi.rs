//! C ABI exports for the Rust shader pipeline.

use std::{
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

use serde::Deserialize as _;

use super::{
    callbacks::CallbackSourceProvider,
    handles::{LAST_ERROR, program_ref, set_last_error},
    request_json::RequestDto,
};
use crate::{
    ShaderError, ShaderResult, ShaderStageKind, compile::NagaCompiler,
    pipeline::DefaultShaderPipeline,
};

/// Successful FFI call status.
pub const RS_SHADER_OK: c_int = 0;
/// Failed FFI call status.
pub const RS_SHADER_ERR: c_int = 1;
/// Vertex stage integer returned to C++.
pub const RS_SHADER_STAGE_VERTEX: c_int = 0;
/// Fragment stage integer returned to C++.
pub const RS_SHADER_STAGE_FRAGMENT: c_int = 1;
/// Unknown stage/index integer returned to C++.
pub const RS_SHADER_STAGE_INVALID: c_int = -1;

/// Include callback result owned by the callback provider.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RsShaderOwnedBytes {
    /// Owned byte pointer returned by the callback.
    pub ptr: *mut u8,
    /// Number of bytes at `ptr`.
    pub len: usize,
    /// Callback-specific deallocator for `ptr`.
    pub free: Option<unsafe extern "C" fn(*mut u8, usize, *mut c_void)>,
    /// User data supplied to `free`.
    pub free_user_data: *mut c_void,
}

/// Include callback used by the C++ bridge.
pub type RsShaderIncludeCallback = extern "C" fn(*const c_char, *mut c_void) -> RsShaderOwnedBytes;

/// Compiles a shader program request encoded as bridge JSON.
///
/// Returns `0` on success and writes a non-null program handle to
/// `out_program`. Returns nonzero on failure and records a thread-local error
/// message retrievable with [`rs_shader_last_error`].
///
/// # Safety
///
/// `request_json` must be a valid, NUL-terminated UTF-8 C string for the
/// duration of the call. `out_program` must be valid for writes. When
/// `include_callback` is provided, it must return either a null pointer with
/// zero length or a valid byte buffer matching `len`; if the returned `free`
/// callback is present, it must be safe to call exactly once with the returned
/// pointer, length, and user data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_compile_program(
    request_json: *const c_char,
    include_callback: Option<RsShaderIncludeCallback>,
    include_user_data: *mut c_void,
    out_program: *mut *mut super::RsShaderProgram,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> ShaderResult<()> {
        if request_json.is_null() {
            return Err(ShaderError::bridge("request json pointer is null"));
        }
        if out_program.is_null() {
            return Err(ShaderError::bridge("out program pointer is null"));
        }

        // SAFETY: Caller guarantees `request_json` is a valid NUL-terminated string.
        let request = unsafe { CStr::from_ptr(request_json) }
            .to_str()
            .map_err(|error| ShaderError::bridge(format!("request json is not utf-8: {error}")))?;
        let request = RequestDto::deserialize(&mut serde_json::Deserializer::from_str(request))
            .map_err(|error| ShaderError::bridge(format!("json request parse failed: {error}")))?
            .into_request()?;
        let provider = CallbackSourceProvider {
            callback: include_callback,
            user_data: include_user_data,
        };
        let program = DefaultShaderPipeline::new(provider, NagaCompiler).compile(&request)?;
        let handle = Box::new(super::RsShaderProgram::try_from(program)?);

        // SAFETY: Caller guarantees `out_program` is valid for writes.
        unsafe {
            *out_program = Box::into_raw(handle);
        }

        Ok(())
    }));

    match result {
        Ok(Ok(())) => {
            set_last_error("no shader error");
            RS_SHADER_OK
        }
        Ok(Err(error)) => {
            set_last_error(error.to_miette_report());
            RS_SHADER_ERR
        }
        Err(_) => {
            set_last_error("shader bridge panicked");
            RS_SHADER_ERR
        }
    }
}

/// Returns the number of compiled stages in `program`.
///
/// Null program handles return `0`.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_stage_count(
    program: *const super::RsShaderProgram,
) -> usize {
    let Some(program) = program_ref(program) else {
        return 0;
    };

    program.program.stages().len()
}

/// Returns the numeric shader stage kind for `stage_index`.
///
/// `0` is vertex, `1` is fragment, and `-1` indicates a null handle or invalid
/// stage index.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_stage_kind(
    program: *const super::RsShaderProgram,
    stage_index: usize,
) -> c_int {
    let Some(stage) =
        program_ref(program).and_then(|program| program.program.stages().get(stage_index))
    else {
        return RS_SHADER_STAGE_INVALID;
    };

    match stage.kind() {
        ShaderStageKind::Vertex => RS_SHADER_STAGE_VERTEX,
        ShaderStageKind::Fragment => RS_SHADER_STAGE_FRAGMENT,
    }
}

/// Returns a borrowed pointer to a compiled stage SPIR-V word buffer.
///
/// The pointer is valid until `program` is freed. Null program handles or
/// invalid stage indices return null.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
/// The returned pointer is borrowed from `program` and must not be used after
/// `rs_shader_program_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_stage_spv_words(
    program: *const super::RsShaderProgram,
    stage_index: usize,
) -> *const u32 {
    program_ref(program)
        .and_then(|program| program.program.stages().get(stage_index))
        .map_or(ptr::null(), |stage| stage.spirv().as_ptr())
}

/// Returns the number of SPIR-V words for a compiled stage.
///
/// Null program handles or invalid stage indices return `0`.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_stage_spv_word_count(
    program: *const super::RsShaderProgram,
    stage_index: usize,
) -> usize {
    program_ref(program)
        .and_then(|program| program.program.stages().get(stage_index))
        .map_or(0, |stage| stage.spirv().len())
}

/// Returns borrowed metadata JSON tied to `program`.
///
/// Null program handles return an empty JSON object.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
/// The returned pointer is borrowed from `program` and must not be used after
/// `rs_shader_program_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_metadata_json(
    program: *const super::RsShaderProgram,
) -> *const c_char {
    program_ref(program).map_or(c"{}".as_ptr(), |program| program.metadata_json.as_ptr())
}

/// Returns borrowed reflection JSON tied to `program`.
///
/// Null program handles return an empty JSON object.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
/// The returned pointer is borrowed from `program` and must not be used after
/// `rs_shader_program_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_reflection_json(
    program: *const super::RsShaderProgram,
) -> *const c_char {
    program_ref(program).map_or(c"{}".as_ptr(), |program| program.reflection_json.as_ptr())
}

/// Returns borrowed diagnostics JSON tied to `program`.
///
/// Null program handles return an empty JSON array.
///
/// # Safety
///
/// `program` must be null or a valid pointer returned by
/// `rs_shader_compile_program` that remains live for the duration of the call.
/// The returned pointer is borrowed from `program` and must not be used after
/// `rs_shader_program_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_diagnostics_json(
    program: *const super::RsShaderProgram,
) -> *const c_char {
    program_ref(program).map_or(c"[]".as_ptr(), |program| program.diagnostics_json.as_ptr())
}

/// Frees a program handle returned by [`rs_shader_compile_program`].
///
/// # Safety
///
/// `program` must be null or a pointer returned by `rs_shader_compile_program`
/// that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_shader_program_free(program: *mut super::RsShaderProgram) {
    if program.is_null() {
        return;
    }

    // SAFETY: Caller guarantees this pointer came from `Box::into_raw` in
    // `rs_shader_compile_program` and has not already been freed.
    drop(unsafe { Box::from_raw(program) });
}

/// Returns the last FFI error message for the current thread.
#[unsafe(no_mangle)]
pub extern "C" fn rs_shader_last_error() -> *const c_char {
    LAST_ERROR.with(|error| error.borrow().as_ptr())
}

/// Forces the Rust linker to retain this FFI module.
#[inline(never)]
pub fn ensure_linked() {
    let _ = std::hint::black_box(rs_shader_compile_program as *const ());
    let _ = std::hint::black_box(rs_shader_program_stage_count as *const ());
    let _ = std::hint::black_box(rs_shader_program_stage_kind as *const ());
    let _ = std::hint::black_box(rs_shader_program_stage_spv_words as *const ());
    let _ = std::hint::black_box(rs_shader_program_stage_spv_word_count as *const ());
    let _ = std::hint::black_box(rs_shader_program_metadata_json as *const ());
    let _ = std::hint::black_box(rs_shader_program_reflection_json as *const ());
    let _ = std::hint::black_box(rs_shader_program_diagnostics_json as *const ());
    let _ = std::hint::black_box(rs_shader_program_free as *const ());
    let _ = std::hint::black_box(rs_shader_last_error as *const ());
}
