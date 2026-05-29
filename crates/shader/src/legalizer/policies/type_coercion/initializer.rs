use super::{
    DeclarationDeclarators, DeclaratorInitializer, Fixup, LocalDeclaration, LocalDeclarationStart,
    PolicyContext, SourceSpan, Token, TokenKind, TokenSearch,
    types::{BindingType, VectorTypeBindings, VectorWidth},
};

/// Local vector declarations initialized from wider visible vector bindings.
pub(super) struct NarrowVectorInitializers<'facts, 'src> {
    /// Shared scoped declaration facts.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
    /// Matching declarations found during the scan.
    pub(super) items: Vec<NarrowVectorInitializer>,
}
impl<'src> NarrowVectorInitializers<'_, 'src> {
    /// Scans tokens in source order for vector declarations.
    pub(super) fn scan(&mut self, tokens: &[Token<'src>]) {
        for index in 0..tokens.len() {
            let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                tokens,
                start: index,
            }) else {
                continue;
            };
            let Some(width) = VectorWidth::from_constructor(declaration.ty()) else {
                continue;
            };
            for declaration in DeclarationDeclarators::new(tokens, declaration) {
                self.collect_declaration(tokens, declaration, width);
            }
        }
    }

    /// Records any required initializer swizzle for a vector declaration.
    pub(super) fn collect_declaration(
        &mut self,
        tokens: &[Token<'src>],
        declaration: LocalDeclaration<'src>,
        width: VectorWidth,
    ) {
        let Some(initializer) = declaration.initializer(tokens) else {
            return;
        };

        self.collect_initializer(tokens, width, initializer);
    }

    /// Emits a narrow-vector initializer swizzle when a vec4 identifier is
    /// assigned.
    pub(super) fn collect_initializer(
        &mut self,
        tokens: &[Token<'src>],
        width: VectorWidth,
        initializer: DeclaratorInitializer,
    ) {
        let Some(swizzle) = width.narrow_swizzle() else {
            return;
        };
        if initializer.start() != initializer.end() {
            return;
        }
        let TokenKind::Identifier(initializer_name) = tokens[initializer.start()].kind else {
            return;
        };
        if !matches!(
            self.facts.lookup(initializer_name, initializer.start()),
            Some(BindingType::Vector(VectorWidth::Four))
        ) {
            return;
        }
        if let Ok(insertion) = SourceSpan::new(
            tokens[initializer.start()].span.end(),
            tokens[initializer.start()].span.end(),
        ) {
            self.items
                .push(NarrowVectorInitializer { insertion, swizzle });
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declaration initialized from an unswizzled wider vector identifier.
pub(super) struct NarrowVectorInitializer {
    /// Source span immediately after the initializer identifier.
    pub(super) insertion: SourceSpan,
    /// Swizzle text to insert.
    pub(super) swizzle: &'static str,
}
impl NarrowVectorInitializer {
    /// Emits the narrowing swizzle insertion.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, self.swizzle.to_owned()));
    }
}
#[derive(Default)]
/// Scalar declarations initialized from visible vector identifiers.
pub(super) struct ScalarVectorInitializers {
    /// Component-selection insertions in source order.
    pub(super) items: Vec<ScalarVectorInitializer>,
}
impl<'src> From<(&[Token<'src>], &VectorTypeBindings<'src>)> for ScalarVectorInitializers {
    fn from((tokens, facts): (&[Token<'src>], &VectorTypeBindings<'src>)) -> Self {
        let mut items = Vec::new();
        for index in 0..tokens.len() {
            let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                tokens,
                start: index,
            }) else {
                continue;
            };
            if declaration.ty() != "float" {
                continue;
            }
            for declaration in DeclarationDeclarators::new(tokens, declaration) {
                let Some(initializer) = declaration.initializer(tokens) else {
                    continue;
                };
                if initializer.start() != initializer.end() {
                    continue;
                }
                let TokenKind::Identifier(name) = tokens[initializer.start()].kind else {
                    continue;
                };
                if !matches!(
                    facts.lookup(name, initializer.start()),
                    Some(BindingType::Vector(_))
                ) {
                    continue;
                }
                let Ok(insertion) = SourceSpan::new(
                    tokens[initializer.start()].span.end(),
                    tokens[initializer.start()].span.end(),
                ) else {
                    continue;
                };
                items.push(ScalarVectorInitializer { insertion });
            }
        }
        Self { items }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Scalar declaration initialized by a vector identifier.
pub(super) struct ScalarVectorInitializer {
    /// Source span immediately after the initializer identifier.
    pub(super) insertion: SourceSpan,
}
impl ScalarVectorInitializer {
    /// Emits the component-selection insertion.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, ".x".to_owned()));
    }
}
#[derive(Default)]
/// Vector declarations whose scalar literal initializers need broadcasting.
pub(super) struct VectorScalarInitializers {
    /// Scalar initializer replacements in source order.
    pub(super) items: Vec<VectorScalarInitializer>,
}
impl VectorScalarInitializers {
    /// Scans vector declarations and records scalar literal initializer spans.
    pub(super) fn scan(&mut self, tokens: &[Token<'_>]) {
        let mut index = 0usize;
        while index < tokens.len() {
            let TokenKind::Identifier(type_name) = tokens[index].kind else {
                index += 1;
                continue;
            };
            let Some(width) = VectorWidth::from_constructor(type_name) else {
                index += 1;
                continue;
            };
            let Some(statement) =
                Option::<VectorDeclarationStatement>::from(VectorDeclarationStart {
                    tokens,
                    type_index: index,
                    width,
                })
            else {
                index += 1;
                continue;
            };
            self.items.extend(statement.initializers());
            index = statement.end + 1;
        }
    }
}
impl<'src> From<&[Token<'src>]> for VectorScalarInitializers {
    fn from(tokens: &[Token<'src>]) -> Self {
        let mut initializers = Self::default();
        initializers.scan(tokens);
        initializers
    }
}
#[derive(Clone, Copy)]
/// Start token for a vector declaration statement.
pub(super) struct VectorDeclarationStart<'tokens, 'src> {
    /// Full token stream being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Token index of the vector type.
    pub(super) type_index: usize,
    /// Declared vector width.
    pub(super) width: VectorWidth,
}
#[derive(Clone, Copy)]
/// Token range for one vector declaration statement.
pub(super) struct VectorDeclarationStatement<'tokens, 'src> {
    /// Full token stream containing this declaration.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token after the vector type.
    pub(super) start: usize,
    /// Semicolon token ending the declaration.
    pub(super) end: usize,
    /// Declared vector width.
    pub(super) width: VectorWidth,
}
impl<'tokens, 'src> From<VectorDeclarationStart<'tokens, 'src>>
    for Option<VectorDeclarationStatement<'tokens, 'src>>
{
    fn from(start: VectorDeclarationStart<'tokens, 'src>) -> Self {
        let tokens = start.tokens;
        let search = TokenSearch::new(tokens);
        let first_name = search.next_non_comment(start.type_index + 1)?;
        if !matches!(tokens[first_name].kind, TokenKind::Identifier(_)) {
            return None;
        }
        let next = search.next_non_comment(first_name + 1)?;
        if !matches!(
            tokens[next].kind,
            TokenKind::Punctuation('=') | TokenKind::Comma | TokenKind::Semicolon
        ) {
            return None;
        }

        let end = StatementEnd {
            tokens,
            start: first_name,
        }
        .semicolon()?;
        Some(VectorDeclarationStatement {
            tokens,
            start: first_name,
            end,
            width: start.width,
        })
    }
}
impl VectorDeclarationStatement<'_, '_> {
    /// Returns scalar initializer replacements contained by this declaration.
    pub(super) fn initializers(self) -> Vec<VectorScalarInitializer> {
        let mut cursor = self.start;
        let mut initializers = Vec::new();
        loop {
            let Some(declarator) = Option::<VectorDeclarator>::from(VectorDeclaratorStart {
                tokens: self.tokens,
                start: cursor,
                end: self.end,
            }) else {
                return initializers;
            };
            if let Some(initializer) = declarator.scalar_initializer(self.width) {
                initializers.push(initializer);
            }
            let Some(next) = declarator.next else {
                return initializers;
            };
            cursor = next;
        }
    }
}
#[derive(Clone, Copy)]
/// Finds the semicolon ending a token statement.
pub(super) struct StatementEnd<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token to scan.
    pub(super) start: usize,
}
impl StatementEnd<'_, '_> {
    /// Returns the first top-level semicolon from the start token.
    pub(super) fn semicolon(self) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(self.start) {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    return Some(index);
                }
                TokenKind::LeftBrace | TokenKind::RightBrace
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return None;
                }
                _ => {}
            }
        }
        None
    }
}
#[derive(Clone, Copy)]
/// Start token for one comma-separated vector declarator.
pub(super) struct VectorDeclaratorStart<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token to inspect.
    pub(super) start: usize,
    /// Semicolon token ending the parent declaration.
    pub(super) end: usize,
}
#[derive(Clone, Copy)]
/// One declarator inside a comma-separated vector declaration statement.
pub(super) struct VectorDeclarator<'tokens, 'src> {
    /// Token after the comma separator, or `None` for the last declarator.
    pub(super) next: Option<usize>,
    /// Initializer token range when this declarator has `=`.
    pub(super) initializer: Option<InitializerTokens<'tokens, 'src>>,
}
impl<'tokens, 'src> From<VectorDeclaratorStart<'tokens, 'src>>
    for Option<VectorDeclarator<'tokens, 'src>>
{
    fn from(start: VectorDeclaratorStart<'tokens, 'src>) -> Self {
        let tokens = start.tokens;
        let search = TokenSearch::new(tokens);
        let name = search.next_non_comment(start.start)?;
        if name >= start.end || !matches!(tokens[name].kind, TokenKind::Identifier(_)) {
            return None;
        }

        let mut cursor = name + 1;
        let mut equals = None;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        while cursor < start.end {
            match tokens[cursor].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TokenKind::Punctuation('=') if paren_depth == 0 && bracket_depth == 0 => {
                    equals = Some(cursor);
                }
                TokenKind::Comma if paren_depth == 0 && bracket_depth == 0 => {
                    let initializer = equals
                        .and_then(|equals| InitializerTokens::between(tokens, equals + 1, cursor));
                    return Some(VectorDeclarator {
                        next: search.next_non_comment(cursor + 1),
                        initializer,
                    });
                }
                _ => {}
            }
            cursor += 1;
        }

        let initializer =
            equals.and_then(|equals| InitializerTokens::between(tokens, equals + 1, start.end));
        Some(VectorDeclarator {
            next: None,
            initializer,
        })
    }
}
impl VectorDeclarator<'_, '_> {
    /// Returns a broadcast fixup when this declarator initializes from a scalar
    /// literal.
    pub(super) fn scalar_initializer(self, width: VectorWidth) -> Option<VectorScalarInitializer> {
        let initializer = ScalarLiteralInitializer::try_from(self.initializer?).ok()?;
        Some(VectorScalarInitializer {
            span: initializer.span,
            width,
        })
    }
}
#[derive(Clone, Copy)]
/// Non-comment initializer token range.
pub(super) struct InitializerTokens<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token index.
    pub(super) start: usize,
    /// Last token index.
    pub(super) end: usize,
}
impl<'tokens, 'src> InitializerTokens<'tokens, 'src> {
    /// Creates an initializer range from raw bounds.
    pub(super) fn between(
        tokens: &'tokens [Token<'src>],
        start: usize,
        end: usize,
    ) -> Option<Self> {
        let search = TokenSearch::new(tokens);
        let start = search.next_non_comment(start)?;
        let end = search.previous_non_comment(end)?;
        (start <= end).then_some(Self { tokens, start, end })
    }
}
#[derive(Clone, Copy)]
/// Scalar literal initializer span.
pub(super) struct ScalarLiteralInitializer {
    /// Source span covering the literal and optional sign.
    pub(super) span: SourceSpan,
}
impl TryFrom<InitializerTokens<'_, '_>> for ScalarLiteralInitializer {
    type Error = ();

    fn try_from(initializer: InitializerTokens<'_, '_>) -> Result<Self, Self::Error> {
        let tokens = initializer.tokens;
        let (start, end) = match (initializer.start, initializer.end) {
            (start, end) if start == end && matches!(tokens[start].kind, TokenKind::Number(_)) => {
                (start, end)
            }
            (start, end)
                if start + 1 == end
                    && matches!(tokens[start].kind, TokenKind::Punctuation('+' | '-'))
                    && matches!(tokens[end].kind, TokenKind::Number(_)) =>
            {
                (start, end)
            }
            _ => return Err(()),
        };
        let span = SourceSpan::new(tokens[start].span.start(), tokens[end].span.end())
            .map_err(|_error| ())?;
        Ok(Self { span })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Vector scalar initializer that needs constructor broadcasting.
pub(super) struct VectorScalarInitializer {
    /// Scalar literal span to replace.
    pub(super) span: SourceSpan,
    /// Constructor width to emit.
    pub(super) width: VectorWidth,
}
impl VectorScalarInitializer {
    /// Emits the scalar-to-vector constructor replacement.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        let source = context.context().module.source();
        let literal = source.slice(self.span);
        context.context().fixups.push(Fixup::replace(
            self.span,
            format!("{}({literal})", self.width.constructor()),
        ));
    }
}
#[derive(Clone, Copy)]
/// Integer literal converted to GLSL float literal spelling.
pub(super) struct FloatLiteral<'src> {
    /// Original literal text.
    pub(super) text: &'src str,
}
impl std::fmt::Display for FloatLiteral<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let trimmed = self.text.trim_end_matches(['u', 'U', 'l', 'L']);
        write!(formatter, "{trimmed}.0")
    }
}
