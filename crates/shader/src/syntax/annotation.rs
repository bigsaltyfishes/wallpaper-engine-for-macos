//! Wallpaper Engine annotation syntax records.

use super::{ShaderModule, ShaderSourceText, source::SpannedSyntax};
use crate::SourceSpan;

/// Wallpaper Engine shader annotation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderAnnotation {
    /// Recognized annotation category.
    kind: AnnotationKind,
    /// Source span covering the full annotation line token.
    span: SourceSpan,
}

impl SpannedSyntax for ShaderAnnotation {
    fn span(&self) -> SourceSpan {
        self.span
    }
}

impl ShaderAnnotation {
    /// Creates an annotation record.
    #[must_use]
    pub const fn new(kind: AnnotationKind, span: SourceSpan) -> Self {
        Self { kind, span }
    }

    /// Creates an annotation record from token text and source span.
    #[must_use]
    pub fn from_token_text(text: &str, span: SourceSpan) -> Self {
        Self {
            kind: AnnotationKind::from_annotation_text(text),
            span,
        }
    }

    /// Returns the annotation category.
    #[must_use]
    pub const fn kind(&self) -> AnnotationKind {
        self.kind
    }

    /// Returns the source span for the annotation line.
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        <Self as SpannedSyntax>::span(self)
    }

    /// Returns annotation text borrowed from the original source.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> &'source str {
        self.text_from(ShaderSourceText::new(source))
    }

    /// Returns annotation text borrowed from a typed source view.
    #[must_use]
    pub fn text_from<'source>(&self, source: ShaderSourceText<'source>) -> &'source str {
        source.slice(self.span)
    }

    /// Returns annotation text borrowed from its parsed module.
    #[must_use]
    pub fn text_in<'source>(&self, module: &ShaderModule<'source>) -> &'source str {
        module.slice(self.span)
    }
}

/// Recognized Wallpaper Engine annotation categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnnotationKind {
    /// `[COMBO]` line annotation.
    Combo,
    /// JSON line annotation.
    Json,
    /// Other Wallpaper Engine bracket annotation.
    Bracket,
}

impl AnnotationKind {
    /// Classifies Wallpaper Engine annotation token text.
    #[must_use]
    pub fn from_annotation_text(text: &str) -> Self {
        let trimmed = text.trim_start_matches('/').trim_start();
        if trimmed.starts_with("[COMBO]") {
            Self::Combo
        } else if trimmed.starts_with('{') {
            Self::Json
        } else {
            Self::Bracket
        }
    }
}
