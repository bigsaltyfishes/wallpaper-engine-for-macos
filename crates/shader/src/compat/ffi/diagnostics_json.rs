//! Bridge diagnostics JSON DTOs.

use crate::ShaderDiagnostic;

/// Diagnostics response JSON.
#[derive(Debug)]
pub(in crate::compat::ffi) struct DiagnosticsJson<'program> {
    /// Diagnostics.
    diagnostics: &'program [ShaderDiagnostic],
}

impl<'program> From<&'program [ShaderDiagnostic]> for DiagnosticsJson<'program> {
    fn from(diagnostics: &'program [ShaderDiagnostic]) -> Self {
        Self { diagnostics }
    }
}

impl serde::Serialize for DiagnosticsJson<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.diagnostics.serialize(serializer)
    }
}
