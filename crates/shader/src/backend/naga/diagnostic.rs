//! Naga diagnostic conversion into crate-native diagnostics.

use crate::{ShaderDiagnostic, ShaderStageKind, SourceSpan};

/// Builder for crate-native diagnostics emitted by Naga integration.
#[derive(Clone, Debug)]
pub(super) struct DiagnosticBuilder {
    /// Shader stage associated with the diagnostic.
    stage: ShaderStageKind,
    /// Compilation pass associated with the diagnostic.
    pass: &'static str,
    /// Generated source path associated with the diagnostic.
    source_path: String,
    /// Human-readable diagnostic message.
    message: String,
    /// Source location supplied by Naga, when available.
    source_location: Option<naga::SourceLocation>,
    /// Generated source text supplied by Naga integration.
    source: Option<String>,
}

impl DiagnosticBuilder {
    /// Creates a diagnostic builder with required context.
    pub(super) fn new(stage: ShaderStageKind, pass: &'static str, source_path: &str) -> Self {
        Self {
            stage,
            pass,
            source_path: source_path.to_owned(),
            message: String::new(),
            source_location: None,
            source: None,
        }
    }

    /// Sets the human-readable message.
    pub(super) fn with_message(mut self, message: String) -> Self {
        self.message = message;
        self
    }

    /// Sets source location context.
    pub(super) fn with_source_location(
        mut self,
        source_location: Option<naga::SourceLocation>,
    ) -> Self {
        self.source_location = source_location;
        self
    }

    /// Sets generated source text context.
    pub(super) fn with_source(mut self, source: &str) -> Self {
        self.source = Some(source.to_owned());
        self
    }

    /// Builds the crate-native diagnostic.
    pub(super) fn build(self) -> ShaderDiagnostic {
        let diagnostic = ShaderDiagnostic::new(self.message)
            .with_stage(self.stage)
            .with_pass(self.pass)
            .with_generated_source_path(self.source_path);
        let diagnostic = if let Some(source) = self.source {
            diagnostic.with_generated_source(source)
        } else {
            diagnostic
        };

        if let Some(location) = self.source_location {
            let span = usize::try_from(location.offset)
                .ok()
                .zip(usize::try_from(location.length).ok())
                .and_then(|(start, length)| {
                    start
                        .checked_add(length)
                        .and_then(|end| SourceSpan::new(start, end).ok())
                });

            match span {
                Some(span) => diagnostic.with_span(span),
                None => diagnostic,
            }
        } else {
            diagnostic
        }
    }
}
