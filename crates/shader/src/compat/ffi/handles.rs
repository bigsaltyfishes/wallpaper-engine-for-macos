//! Owned FFI handles.

use std::{cell::RefCell, ffi::CString};

use super::{
    diagnostics_json::DiagnosticsJson,
    response_json::{MetadataJson, ReflectionJson},
};
use crate::{CompiledShaderProgram, ShaderError};

thread_local! {
    /// Thread-local error text exposed by `rs_shader_last_error`.
    pub(in crate::compat::ffi) static LAST_ERROR: RefCell<CString> = RefCell::new(cstring_lossy("no shader error"));
}

/// Opaque compiled shader program handle returned to C++.
#[derive(Debug)]
pub struct RsShaderProgram {
    /// Compiled shader program retained by this handle.
    pub(in crate::compat::ffi) program: CompiledShaderProgram,
    /// Prepared metadata JSON borrowed by accessors.
    pub(in crate::compat::ffi) metadata_json: CString,
    /// Prepared reflection JSON borrowed by accessors.
    pub(in crate::compat::ffi) reflection_json: CString,
    /// Prepared diagnostics JSON borrowed by accessors.
    pub(in crate::compat::ffi) diagnostics_json: CString,
}

impl TryFrom<CompiledShaderProgram> for RsShaderProgram {
    type Error = ShaderError;

    fn try_from(program: CompiledShaderProgram) -> Result<Self, Self::Error> {
        let metadata_json = cstring_lossy(
            serde_json::to_string(&MetadataJson::from(program.metadata()))
                .map_err(|error| ShaderError::bridge(error.to_string()))?,
        );
        let reflection_json = cstring_lossy(
            serde_json::to_string(&ReflectionJson::from(program.reflection()))
                .map_err(|error| ShaderError::bridge(error.to_string()))?,
        );
        let diagnostics_json = cstring_lossy(
            serde_json::to_string(&DiagnosticsJson::from(program.diagnostics()))
                .map_err(|error| ShaderError::bridge(error.to_string()))?,
        );

        Ok(Self {
            program,
            metadata_json,
            reflection_json,
            diagnostics_json,
        })
    }
}

/// Records a thread-local FFI error string.
pub(in crate::compat::ffi) fn set_last_error(message: impl Into<String>) {
    let message = cstring_lossy(message.into());
    LAST_ERROR.with(|last_error| {
        *last_error.borrow_mut() = message;
    });
}

/// Returns a borrowed program handle when `program` is non-null.
pub(in crate::compat::ffi) fn program_ref<'program>(
    program: *const RsShaderProgram,
) -> Option<&'program RsShaderProgram> {
    if program.is_null() {
        None
    } else {
        // SAFETY: The non-null pointer is treated as borrowed. Invalid external
        // pointers remain caller UB per the FFI contract.
        Some(unsafe { &*program })
    }
}

/// Creates a C string, replacing interior NUL bytes to preserve FFI validity.
pub(in crate::compat::ffi) fn cstring_lossy(message: impl Into<Vec<u8>>) -> CString {
    let bytes = message
        .into()
        .into_iter()
        .map(|byte| if byte == 0 { b' ' } else { byte })
        .collect::<Vec<_>>();
    CString::new(bytes).unwrap_or_else(|_| c"invalid shader string".to_owned())
}
