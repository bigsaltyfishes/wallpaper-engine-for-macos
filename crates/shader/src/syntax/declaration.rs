//! Top-level declaration syntax records.

use super::{ShaderAnnotation, ShaderModule, ShaderSourceText, source::SpannedSyntax};
use crate::SourceSpan;

/// Top-level declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderDeclaration<'src> {
    /// Declaration category inferred by the lightweight parser.
    kind: DeclarationKind,
    /// Top-level interface qualifier, when present.
    qualifier: Option<TopLevelQualifier>,
    /// Borrowed declaration type token, when known.
    type_name: Option<&'src str>,
    /// Borrowed declaration identifier token, when known.
    name: Option<&'src str>,
    /// Array suffix on the declared identifier, when known.
    array_suffix: Option<DeclarationArraySuffix<'src>>,
    /// Leading layout qualifier, when present.
    layout: Option<DeclarationLayout<'src>>,
    /// Source span covering the full declaration.
    span: SourceSpan,
}

impl<'src> ShaderDeclaration<'src> {
    /// Creates a declaration record.
    #[must_use]
    pub fn new(
        kind: DeclarationKind,
        qualifier: Option<TopLevelQualifier>,
        type_name: Option<&'src str>,
        name: Option<&'src str>,
        array_suffix: Option<DeclarationArraySuffix<'src>>,
        layout: Option<DeclarationLayout<'src>>,
        span: SourceSpan,
    ) -> Self {
        Self {
            kind,
            qualifier,
            type_name,
            name,
            array_suffix,
            layout,
            span,
        }
    }

    /// Returns the declaration kind.
    #[must_use]
    pub const fn kind(&self) -> DeclarationKind {
        self.kind
    }

    /// Returns the top-level qualifier when present.
    #[must_use]
    pub const fn qualifier(&self) -> Option<TopLevelQualifier> {
        self.qualifier
    }

    /// Returns the declared type name when known.
    #[must_use]
    pub const fn type_name(&self) -> Option<&'src str> {
        self.type_name
    }

    /// Returns the declared type fact when known.
    #[must_use]
    pub fn declaration_type(&self) -> Option<DeclarationType<'src>> {
        <Self as DeclarationFacts<'src>>::declaration_type(self)
    }

    /// Returns the declared identifier when known.
    #[must_use]
    pub const fn name(&self) -> Option<&'src str> {
        self.name
    }

    /// Returns the declared identifier fact when known.
    #[must_use]
    pub fn declaration_name(&self) -> Option<DeclarationName<'src>> {
        <Self as DeclarationFacts<'src>>::declaration_name(self)
    }

    /// Returns the array suffix on the declared identifier, when known.
    #[must_use]
    pub fn array_suffix(&self) -> Option<DeclarationArraySuffix<'src>> {
        <Self as DeclarationFacts<'src>>::declaration_array_suffix(self)
    }

    /// Returns the leading layout qualifier, when present.
    #[must_use]
    pub fn layout(&self) -> Option<DeclarationLayout<'src>> {
        <Self as DeclarationFacts<'src>>::declaration_layout(self)
    }

    /// Returns the full declaration source span.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns declaration text borrowed from the original source.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> &'source str {
        self.text_from(ShaderSourceText::new(source))
    }

    /// Returns declaration text borrowed from a typed source view.
    #[must_use]
    pub fn text_from<'source>(&self, source: ShaderSourceText<'source>) -> &'source str {
        source.slice(self.span)
    }

    /// Returns declaration text borrowed from its parsed module.
    #[must_use]
    pub fn text_in(&self, module: &ShaderModule<'src>) -> &'src str {
        module.slice(self.span)
    }

    /// Returns whether `annotation` trails this declaration without crossing a
    /// line boundary.
    #[must_use]
    pub fn has_same_line_annotation(
        &self,
        module: &ShaderModule<'src>,
        annotation: &ShaderAnnotation,
    ) -> bool {
        module.source().is_same_line_gap(
            <Self as SpannedSyntax>::span(self),
            <ShaderAnnotation as SpannedSyntax>::span(annotation),
        )
    }
}

impl<'src> DeclarationFacts<'src> for ShaderDeclaration<'src> {
    fn declaration_name(&self) -> Option<DeclarationName<'src>> {
        self.name.map(|source| DeclarationName { source })
    }

    fn declaration_type(&self) -> Option<DeclarationType<'src>> {
        self.type_name.map(|source| DeclarationType { source })
    }

    fn declaration_array_suffix(&self) -> Option<DeclarationArraySuffix<'src>> {
        self.array_suffix
    }

    fn declaration_layout(&self) -> Option<DeclarationLayout<'src>> {
        self.layout
    }
}

impl SpannedSyntax for ShaderDeclaration<'_> {
    fn span(&self) -> SourceSpan {
        self.span()
    }
}

/// Strongly typed declaration identifier fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationName<'src> {
    /// Borrowed identifier text.
    source: &'src str,
}

impl<'src> DeclarationName<'src> {
    /// Returns the identifier text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }
}

/// Strongly typed declaration type fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationType<'src> {
    /// Borrowed type token text.
    source: &'src str,
}

impl<'src> DeclarationType<'src> {
    /// Returns the type token text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }
}

/// Strongly typed array suffix fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationArraySuffix<'src> {
    /// Borrowed suffix text, including brackets.
    pub(super) source: &'src str,
}

impl<'src> DeclarationArraySuffix<'src> {
    /// Returns the suffix text, including brackets.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }
}

/// Strongly typed layout qualifier fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationLayout<'src> {
    /// Borrowed layout qualifier text.
    pub(super) source: &'src str,
}

impl<'src> DeclarationLayout<'src> {
    /// Returns the layout qualifier text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }
}

/// Shared declaration facts exposed by parsed declarations.
pub trait DeclarationFacts<'src> {
    /// Returns the declared identifier fact when known.
    fn declaration_name(&self) -> Option<DeclarationName<'src>>;

    /// Returns the declared type fact when known.
    fn declaration_type(&self) -> Option<DeclarationType<'src>>;

    /// Returns the declared array suffix when known.
    fn declaration_array_suffix(&self) -> Option<DeclarationArraySuffix<'src>>;

    /// Returns the leading layout qualifier when present.
    fn declaration_layout(&self) -> Option<DeclarationLayout<'src>>;
}

/// Declaration category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationKind {
    /// Interface declaration such as `uniform`, `in`, or `out`.
    Interface,
    /// Struct declaration.
    Struct,
    /// Other semicolon-terminated top-level declaration.
    Other,
}

/// Recognized top-level GLSL interface qualifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TopLevelQualifier {
    /// `uniform`.
    Uniform,
    /// `attribute`.
    Attribute,
    /// `varying`.
    Varying,
    /// `in`.
    In,
    /// `out`.
    Out,
}
