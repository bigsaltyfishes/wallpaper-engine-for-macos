//! C include callback source provider.

use std::{os::raw::c_void, slice};

use super::{
    abi::{RsShaderIncludeCallback, RsShaderOwnedBytes},
    handles::cstring_lossy,
};
use crate::{IncludePath, ShaderError};

/// Source provider backed by a C include callback.
#[derive(Clone, Copy, Debug)]
pub(in crate::compat::ffi) struct CallbackSourceProvider {
    /// Optional include callback.
    pub(in crate::compat::ffi) callback: Option<RsShaderIncludeCallback>,
    /// Opaque callback user data.
    pub(in crate::compat::ffi) user_data: *mut c_void,
}

impl crate::ShaderSourceProvider for CallbackSourceProvider {
    fn read_to_string(&self, path: &IncludePath) -> crate::ShaderResult<String> {
        let Some(callback) = self.callback else {
            return Err(ShaderError::IncludeNotFound { path: path.clone() });
        };
        let path_text = cstring_lossy(path.as_str());
        let bytes = callback(path_text.as_ptr(), self.user_data);
        CallbackBytes {
            path: path.clone(),
            bytes,
        }
        .into_string()
    }
}

/// RAII owner for bytes returned by a C include callback.
#[derive(Debug)]
pub(in crate::compat::ffi) struct CallbackBytes {
    /// Include path associated with these bytes.
    path: IncludePath,
    /// Raw callback bytes and free callback.
    bytes: RsShaderOwnedBytes,
}

impl CallbackBytes {
    /// Copies callback bytes into a Rust string.
    fn into_string(self) -> crate::ShaderResult<String> {
        if self.bytes.ptr.is_null() {
            if self.bytes.len == 0 {
                return Err(ShaderError::IncludeNotFound {
                    path: self.path.clone(),
                });
            }
            return Err(ShaderError::source_read(
                self.path.clone(),
                "include callback returned null pointer with nonzero length",
            ));
        }

        // SAFETY: The callback contract requires `ptr` to reference `len`
        // readable bytes until the matching free callback is called.
        let copied =
            unsafe { slice::from_raw_parts(self.bytes.ptr.cast_const(), self.bytes.len) }.to_vec();

        String::from_utf8(copied).map_err(|_| ShaderError::invalid_source_utf8(self.path.clone()))
    }
}

impl Drop for CallbackBytes {
    fn drop(&mut self) {
        if let Some(free) = self.bytes.free
            && !self.bytes.ptr.is_null()
        {
            // SAFETY: The callback returned this pointer/free pair and Rust
            // calls it exactly once from this RAII owner.
            unsafe { free(self.bytes.ptr, self.bytes.len, self.bytes.free_user_data) };
        }
    }
}
