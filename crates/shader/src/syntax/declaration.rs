//! Top-level declaration syntax records.

use smol_str::SmolStr;

use super::{ShaderAnnotation, ShaderModule, ShaderSourceText, source::SpannedSyntax};
use crate::SourceSpan;

/// Top-level declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderDeclaration<'src> {
    /// Declaration category inferred by the lightweight parser.
    kind: DeclarationKind,
    /// Top-level interface qualifier, when present.
    qualifier: Option<TopLevelQualifier>,
    /// Declaration type token, when known.
    type_name: Option<SmolStr>,
    /// Declaration identifier token, when known.
    name: Option<SmolStr>,
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
        type_name: Option<SmolStr>,
        name: Option<SmolStr>,
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
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }

    /// Returns the declared type fact when known.
    #[must_use]
    pub fn declaration_type(&self) -> Option<DeclarationType> {
        <Self as DeclarationFacts>::declaration_type(self)
    }

    /// Returns the declared identifier when known.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the declared identifier fact when known.
    #[must_use]
    pub fn declaration_name(&self) -> Option<DeclarationName> {
        <Self as DeclarationFacts>::declaration_name(self)
    }

    /// Returns the declared identifier fact, including parser fallback facts
    /// owned by the containing module.
    #[must_use]
    pub fn declaration_name_in(&self, module: &ShaderModule<'src>) -> Option<DeclarationName> {
        self.declaration_name().or_else(|| {
            module
                .first_declarator_name(self)
                .map(|source| DeclarationName { source })
        })
    }

    /// Returns the array suffix on the declared identifier, when known.
    #[must_use]
    pub fn array_suffix(&self) -> Option<DeclarationArraySuffix<'src>> {
        <Self as DeclarationFacts>::declaration_array_suffix(self)
    }

    /// Returns the leading layout qualifier, when present.
    #[must_use]
    pub fn layout(&self) -> Option<DeclarationLayout<'src>> {
        <Self as DeclarationFacts>::declaration_layout(self)
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
    fn declaration_name(&self) -> Option<DeclarationName> {
        self.name.as_ref().map(|source| DeclarationName {
            source: source.clone(),
        })
    }

    fn declaration_type(&self) -> Option<DeclarationType> {
        self.type_name.as_ref().map(|source| DeclarationType {
            source: source.clone(),
        })
    }

    fn declaration_array_suffix(&self) -> Option<DeclarationArraySuffix<'src>> {
        self.array_suffix.clone()
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationName {
    /// Identifier text.
    source: SmolStr,
}

impl DeclarationName {
    /// Returns the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.source.as_str()
    }
}

/// Strongly typed declaration type fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationType {
    /// Type token text.
    source: SmolStr,
}

impl DeclarationType {
    /// Returns the type token text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.source.as_str()
    }
}

/// Strongly typed array suffix fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationArraySuffix<'src> {
    /// Borrowed suffix text, including brackets.
    pub source: &'src str,
    /// Parsed array size expression, when the suffix is supported by resource
    /// planning.
    pub size: Option<DeclarationArraySize>,
}

impl<'src> DeclarationArraySuffix<'src> {
    /// Returns the suffix text, including brackets.
    #[must_use]
    pub const fn as_str(&self) -> &'src str {
        self.source
    }

    /// Returns the parsed array size expression.
    #[must_use]
    pub fn size(&self) -> Option<DeclarationArraySize> {
        self.size.clone()
    }
}

/// Strongly typed layout qualifier fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationLayout<'src> {
    /// Borrowed layout qualifier text.
    pub source: &'src str,
    /// Descriptor set index in the first layout qualifier declaring a binding.
    pub set: Option<u32>,
    /// Descriptor binding index in the first layout qualifier declaring one.
    pub binding: Option<u32>,
}

impl<'src> DeclarationLayout<'src> {
    /// Returns the layout qualifier text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }

    /// Returns the descriptor set index declared with the binding, when any.
    #[must_use]
    pub const fn set(self) -> Option<u32> {
        self.set
    }

    /// Returns the descriptor binding index, when any.
    #[must_use]
    pub const fn binding(self) -> Option<u32> {
        self.binding
    }
}

/// Parsed declaration array size expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationArraySize {
    /// Literal numeric array length.
    Numeric(u32),
    /// Macro identifier array length.
    MacroIdentifier(SmolStr),
}

/// Shared declaration facts exposed by parsed declarations.
pub(super) trait DeclarationFacts<'src> {
    /// Returns the declared identifier fact when known.
    fn declaration_name(&self) -> Option<DeclarationName>;

    /// Returns the declared type fact when known.
    fn declaration_type(&self) -> Option<DeclarationType>;

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
