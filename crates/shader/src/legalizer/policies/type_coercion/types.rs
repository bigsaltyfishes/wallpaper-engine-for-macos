use super::{
    ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, SourceSpan,
    Token, TokenKind, TokenSearch,
    assignment::{AssignmentRhs, SwizzledExpression},
};

#[derive(Default)]
/// Known scalar and vector declaration facts.
pub(super) struct VectorTypeBindings<'src> {
    /// Bindings in source order.
    pub(super) bindings: Vec<TypeBinding<'src>>,
}
impl<'src> From<&[Token<'src>]> for VectorTypeBindings<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let facts = ScopedDeclarationFacts::from_tokens(
            tokens,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );
        Self {
            bindings: facts
                .declarations()
                .iter()
                .map(|declaration| TypeBinding {
                    name: declaration.name(),
                    ty: BindingType::from(declaration.ty()),
                    visible_start: declaration.visible_start(),
                    scope_end: declaration.scope_end(),
                })
                .collect(),
        }
    }
}
impl VectorTypeBindings<'_> {
    /// Looks up the nearest visible binding by name at `use_index`.
    pub(super) fn lookup(&self, name: &str, use_index: usize) -> Option<BindingType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(use_index))
            .map(|binding| binding.ty)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Vector swizzle field on a member access expression.
pub(super) struct SwizzleField {
    /// Width implied by the field component count.
    pub(super) width: VectorWidth,
}
impl SwizzleField {
    /// Parses a vector swizzle field from component text.
    pub(super) fn parse(field: impl AsRef<str>) -> Result<Self, ()> {
        let field = field.as_ref();
        let width = if !(2..=4).contains(&field.len())
            || !field.bytes().all(|byte| {
                matches!(
                    byte,
                    b'x' | b'y'
                        | b'z'
                        | b'w'
                        | b'r'
                        | b'g'
                        | b'b'
                        | b'a'
                        | b's'
                        | b't'
                        | b'p'
                        | b'q'
                )
            }) {
            None
        } else {
            match field.len() {
                2 => Some(VectorWidth::Two),
                3 => Some(VectorWidth::Three),
                4 => Some(VectorWidth::Four),
                _ => None,
            }
        }
        .ok_or(())?;
        Ok(Self { width })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token range whose final member access is a vector swizzle.
pub(super) struct SwizzledTokenRange {
    /// Base identifier token index.
    pub(super) base: usize,
    /// Width produced by the final swizzle.
    pub(super) width: VectorWidth,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Inclusive token range.
pub(super) struct TokenRange<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token in the range.
    pub(super) start: usize,
    /// Last token in the range.
    pub(super) end: usize,
}
/// Shared facts for inclusive token ranges.
pub(super) trait TokenRangeFacts {
    /// First token in the range.
    fn start(self) -> usize;

    /// Last token in the range.
    fn end(self) -> usize;
}
impl TokenRangeFacts for TokenRange<'_, '_> {
    fn start(self) -> usize {
        self.start
    }

    fn end(self) -> usize {
        self.end
    }
}
impl TryFrom<TokenRange<'_, '_>> for SwizzledTokenRange {
    type Error = ();

    fn try_from(range: TokenRange<'_, '_>) -> Result<Self, Self::Error> {
        let TokenKind::Identifier(field) = range.tokens[range.end()].kind else {
            return Err(());
        };
        let field = SwizzleField::parse(field)?;
        let search = TokenSearch::new(range.tokens);
        let dot = search.previous_non_comment(range.end()).ok_or(())?;
        if !matches!(range.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return Err(());
        }
        let base = search.previous_non_comment(dot).ok_or(())?;
        if base != range.start() {
            return Err(());
        }
        if !matches!(range.tokens[base].kind, TokenKind::Identifier(_)) {
            return Err(());
        }
        Ok(Self {
            base,
            width: field.width,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Supported vector widths.
pub(super) enum VectorWidth {
    /// `vec2`.
    Two,
    /// `vec3`.
    Three,
    /// `vec4`.
    Four,
}
impl VectorWidth {
    /// Classifies a vector constructor/type name.
    pub(super) const fn from_constructor(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"vec2" | b"float2" => Some(Self::Two),
            b"vec3" | b"float3" => Some(Self::Three),
            b"vec4" | b"float4" => Some(Self::Four),
            _ => None,
        }
    }

    /// Returns GLSL constructor spelling.
    pub(super) const fn constructor(self) -> &'static str {
        match self {
            Self::Two => "vec2",
            Self::Three => "vec3",
            Self::Four => "vec4",
        }
    }

    /// Returns component swizzle needed to narrow from vec4.
    pub(super) const fn narrow_swizzle(self) -> Option<&'static str> {
        match self {
            Self::Two => Some(".xy"),
            Self::Three => Some(".xyz"),
            Self::Four => None,
        }
    }

    /// Returns a swizzle that selects this many components.
    pub(super) const fn component_swizzle(self) -> &'static str {
        match self {
            Self::Two => ".xy",
            Self::Three => ".xyz",
            Self::Four => ".xyzw",
        }
    }
}
impl PartialOrd for VectorWidth {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for VectorWidth {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.component_count().cmp(&other.component_count())
    }
}
impl VectorWidth {
    /// Returns the number of vector components.
    pub(super) const fn component_count(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}
/// Shared facts for expressions that can resolve to vector width.
pub(super) trait ExpressionWidthFacts {
    /// Returns the vector width produced by this expression.
    fn vector_width(self) -> Option<VectorWidth>;
}
#[derive(Clone, Copy)]
/// One scalar or vector binding.
pub(super) struct TypeBinding<'src> {
    /// Variable name.
    pub(super) name: &'src str,
    /// Declared scalar or vector type.
    pub(super) ty: BindingType,
    /// First token where this binding is visible.
    pub(super) visible_start: usize,
    /// First token outside this binding's lexical scope.
    pub(super) scope_end: usize,
}
impl TypeBinding<'_> {
    /// Returns whether this binding is visible at `use_index`.
    pub(super) const fn visible_at(&self, use_index: usize) -> bool {
        self.visible_start <= use_index && use_index < self.scope_end
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Scalar/vector shape for one visible declaration.
pub(super) enum BindingType {
    /// Scalar or otherwise non-vector local type.
    Scalar,
    /// Vector declaration width.
    Vector(VectorWidth),
    /// Aggregate declaration that shadows an outer scalar/vector name.
    Blocker,
}
impl From<&str> for BindingType {
    /// Classifies declaration type spelling into scalar or vector shape.
    fn from(name: &str) -> Self {
        if let Some(width) = VectorWidth::from_constructor(name) {
            Self::Vector(width)
        } else if matches!(
            name.as_bytes(),
            b"bool" | b"int" | b"uint" | b"float" | b"float1"
        ) {
            Self::Scalar
        } else {
            Self::Blocker
        }
    }
}
impl AssignmentRhs<'_, '_> {
    /// Returns the vector width produced by this RHS expression.
    pub(super) fn width(self) -> Option<VectorWidth> {
        if let Some(width) = SwizzledExpression::try_from(self)
            .ok()
            .map(|swizzled| swizzled.width)
        {
            return Some(width);
        }
        let TokenKind::Identifier(name) = self.tokens[self.start].kind else {
            return None;
        };
        let width = VectorWidth::from_constructor(name)?;
        let open = TokenSearch::new(self.tokens).next_non_comment(self.start + 1)?;
        matches!(
            (self.tokens[open].kind, self.tokens[self.end].kind),
            (TokenKind::LeftParen, TokenKind::RightParen)
        )
        .then_some(width)
    }
}
#[derive(Clone, Copy)]
/// Legacy vector spelling normalized to GLSL constructors.
pub(super) struct LegacyVectorTypeName<'src> {
    /// Source type name.
    pub(super) ty: &'src str,
}
impl<'src> LegacyVectorTypeName<'src> {
    /// Returns the GLSL spelling for known legacy vector types.
    pub(super) const fn glsl(self) -> &'src str {
        match self.ty.as_bytes() {
            b"float2" => "vec2",
            b"float3" => "vec3",
            b"float4" => "vec4",
            _ => self.ty,
        }
    }
}
/// Source-span insertion helpers.
pub(super) trait SourcePointExt {
    /// Returns a zero-width span at the start of this span.
    fn start_point(self) -> SourceSpan;
    /// Returns a zero-width span at the end of this span.
    fn end_point(self) -> SourceSpan;
}
impl SourcePointExt for SourceSpan {
    fn start_point(self) -> SourceSpan {
        SourceSpan::new(self.start(), self.start()).unwrap_or(self)
    }

    fn end_point(self) -> SourceSpan {
        SourceSpan::new(self.end(), self.end()).unwrap_or(self)
    }
}
