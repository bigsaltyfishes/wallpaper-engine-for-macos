//! Shader diagnostic records.

use super::{
    SourceSpan,
    report::{MietteReport, ReportContext},
};
use crate::model::ShaderStageKind;

/// Diagnostic information produced while handling a shader.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderDiagnostic {
    /// Shader stage associated with the diagnostic, when known.
    stage: Option<ShaderStageKind>,
    /// Pipeline or legalization pass associated with the diagnostic.
    pass: Option<String>,
    /// Source range associated with the diagnostic.
    span: Option<SourceSpan>,
    /// Human-readable diagnostic message.
    message: String,
    /// Path to generated source associated with the diagnostic.
    generated_source_path: Option<String>,
    /// Generated source text associated with the diagnostic.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing))]
    generated_source: Option<String>,
}

impl ShaderDiagnostic {
    /// Creates a diagnostic with a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            stage: None,
            pass: None,
            span: None,
            message: message.into(),
            generated_source_path: None,
            generated_source: None,
        }
    }

    /// Sets the shader stage context.
    #[must_use]
    pub fn with_stage(mut self, stage: ShaderStageKind) -> Self {
        self.stage = Some(stage);
        self
    }

    /// Sets the legalization or pipeline pass context.
    #[must_use]
    pub fn with_pass(mut self, pass: impl Into<String>) -> Self {
        self.pass = Some(pass.into());
        self
    }

    /// Sets the source span context.
    #[must_use]
    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// Sets the generated source path context.
    #[must_use]
    pub fn with_generated_source_path(mut self, path: impl Into<String>) -> Self {
        self.generated_source_path = Some(path.into());
        self
    }

    /// Sets generated source text for structured report rendering.
    #[must_use]
    pub fn with_generated_source(mut self, source: impl Into<String>) -> Self {
        self.generated_source = Some(source.into());
        self
    }

    /// Returns the shader stage context.
    #[must_use]
    pub const fn stage(&self) -> Option<ShaderStageKind> {
        self.stage
    }

    /// Returns the pass context.
    #[must_use]
    pub fn pass(&self) -> Option<&str> {
        self.pass.as_deref()
    }

    /// Returns the source span context.
    #[must_use]
    pub const fn span(&self) -> Option<SourceSpan> {
        self.span
    }

    /// Returns the diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the generated source path context.
    #[must_use]
    pub fn generated_source_path(&self) -> Option<&str> {
        self.generated_source_path.as_deref()
    }

    /// Renders this diagnostic with `miette` using optional source text.
    #[must_use]
    pub fn to_miette_report(&self, source: Option<&str>) -> String {
        MietteReport::from(ReportContext {
            diagnostic: self,
            source: self.generated_source.as_deref().or(source),
        })
        .render()
    }
}
