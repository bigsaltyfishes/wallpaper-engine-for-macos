//! Alpha-to-coverage derivative idiom legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult, ShaderStageKind, SourceSpan,
    legalizer::{
        ExpressionReplacement, Fixup, FunctionCallIndex, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, TokenSearch,
    },
    lexer::{Token, TokenKind},
};

/// Replaces legacy alpha-to-coverage derivative sharpening with the
/// pre-derivative color alpha.
struct AlphaToCoveragePolicy;

#[distributed_slice(GENERAL_POLICIES)]
static ALPHA_TO_COVERAGE_POLICY: &dyn Emitable = &AlphaToCoveragePolicy;

impl Emitable for AlphaToCoveragePolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        if context.context().module.stage() != ShaderStageKind::Fragment {
            return Ok(());
        }

        let tokens = context.context().module.tokens();
        let fragment_color_sources = FragmentColorSources::from(tokens);
        for assignment in AlphaAssignments::from(tokens) {
            let Some(source) = fragment_color_sources.visible_source_before(assignment.start)
            else {
                continue;
            };
            let source_alpha = ExpressionReplacement::new()
                .with_source(source.assigned_span)
                .with_text(".a");
            let replacement = if assignment.clamp {
                ExpressionReplacement::new()
                    .with_text("clamp(")
                    .with_replacement(source_alpha)
                    .with_text(", 0.0, 1.0)")
            } else {
                source_alpha
            };
            context
                .context()
                .fixups
                .push(Fixup::replace(assignment.span, replacement));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Alpha assignment matching the derivative idiom.
struct AlphaAssignment {
    /// Matched RHS span to replace.
    span: SourceSpan,
    /// Whether the matched expression came from a saturate wrapper.
    clamp: bool,
    /// First assignment token.
    start: usize,
}

/// Iterator over matching alpha-to-coverage assignments.
struct AlphaAssignments<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [Token<'src>],
    /// Next token index to inspect.
    cursor: usize,
}

impl<'tokens, 'src> From<&'tokens [Token<'src>]> for AlphaAssignments<'tokens, 'src> {
    fn from(tokens: &'tokens [Token<'src>]) -> Self {
        Self { tokens, cursor: 0 }
    }
}

impl Iterator for AlphaAssignments<'_, '_> {
    type Item = AlphaAssignment;

    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < self.tokens.len() {
            let index = self.cursor;
            self.cursor += 1;
            let tokens = self.tokens;
            let Some(lvalue) = AlphaLvalue::from(tokens, index) else {
                continue;
            };
            if !lvalue.frag_color {
                continue;
            }
            let search = TokenSearch::new(tokens);
            let Some(equals) = search.next_non_comment(lvalue.end + 1) else {
                continue;
            };
            if !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
                continue;
            }
            let Some(rhs_start) = search.next_non_comment(equals + 1) else {
                continue;
            };
            let inner = FunctionCallIndex::new(tokens)
                .iter()
                .find(|call| call.name_index == rhs_start && call.name() == "saturate")
                .and_then(|call| {
                    let argument = call.first_argument()?;
                    let start = argument.start();
                    let end = TokenSearch::new(tokens).previous_non_comment(call.close_index)?;
                    Some((start, end, call.span(), true))
                })
                .or_else(|| {
                    let mut paren_depth = 0usize;
                    let mut bracket_depth = 0usize;
                    let mut semicolon = None;
                    for (index, token) in tokens.iter().enumerate().skip(rhs_start) {
                        match token.kind {
                            TokenKind::LeftParen => paren_depth += 1,
                            TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                            TokenKind::Punctuation('[') => bracket_depth += 1,
                            TokenKind::Punctuation(']') => {
                                bracket_depth = bracket_depth.checked_sub(1)?;
                            }
                            TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                                semicolon = Some(index);
                                break;
                            }
                            _ => {}
                        }
                    }
                    let end = TokenSearch::new(tokens).previous_non_comment(semicolon?)?;
                    let span =
                        SourceSpan::new(tokens[rhs_start].span.start(), tokens[end].span.end())
                            .ok()?;
                    (rhs_start <= end).then_some((rhs_start, end, span, false))
                });
            let Some((inner_start, inner_end, span, clamp)) = inner else {
                continue;
            };
            let alpha_references = (inner_start..=inner_end)
                .filter(|index| {
                    AlphaLvalue::from(tokens, *index).is_some_and(|lvalue| lvalue.frag_color)
                })
                .count();
            let derivative_idiom = alpha_references >= 2
                && tokens[inner_start..=inner_end].iter().any(|token| {
                    matches!(
                        token.kind,
                        TokenKind::Identifier("fwidth" | "dFdx" | "dFdy" | "ddx" | "ddy")
                    )
                })
                && tokens[inner_start..=inner_end]
                    .iter()
                    .any(|token| matches!(token.kind, TokenKind::Identifier("max")))
                && tokens[inner_start..=inner_end]
                    .iter()
                    .filter(|token| matches!(token.kind, TokenKind::Number("0.5")))
                    .count()
                    >= 2;
            if !derivative_idiom {
                continue;
            }
            return Some(AlphaAssignment {
                span,
                clamp,
                start: index,
            });
        }
        None
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Visible identifier assignments into the fragment color output.
struct FragmentColorSources<'src> {
    /// Source assignments in token order.
    assignments: Vec<FragmentColorSource<'src>>,
    /// Scoped vector declaration facts.
    declarations: Vec<VectorDeclaration<'src>>,
}

impl<'src> From<&[Token<'src>]> for FragmentColorSources<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let declarations: Vec<VectorDeclaration<'src>> = ScopedDeclarationFacts::from_tokens(
            tokens,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Builtins,
                local_types: ScopedDeclarationTypeMode::Builtins,
            },
        )
        .declarations()
        .iter()
        .filter_map(|declaration| {
            matches!(declaration.ty(), "vec4" | "float4").then_some(VectorDeclaration {
                name: declaration.name(),
                visible_start: declaration.visible_start(),
                scope_end: declaration.scope_end(),
            })
        })
        .collect();

        let assignments = FragmentColorSourceAssignments::from(tokens)
            .filter_map(|assignment| {
                let declaration = declarations
                    .iter()
                    .rev()
                    .find(|declaration| {
                        declaration.name == assignment.name
                            && declaration.visible_at(assignment.index)
                    })
                    .copied()?;
                Some(FragmentColorSource {
                    index: assignment.index,
                    assigned_span: assignment.span,
                    declaration,
                })
            })
            .collect();

        Self {
            assignments,
            declarations,
        }
    }
}

impl<'src> FragmentColorSources<'src> {
    /// Returns the nearest visible identifier assigned to the fragment color
    /// before `before`.
    fn visible_source_before(&self, before: usize) -> Option<FragmentColorSource<'src>> {
        self.assignments
            .iter()
            .rev()
            .find(|assignment| {
                assignment.index < before
                    && self.visible_declaration(assignment.declaration, before)
            })
            .copied()
    }

    /// Returns whether `declaration` is the visible vec4 binding at `index`.
    fn visible_declaration(&self, declaration: VectorDeclaration<'src>, index: usize) -> bool {
        self.declarations
            .iter()
            .rev()
            .find(|candidate| candidate.name == declaration.name && candidate.visible_at(index))
            .is_some_and(|candidate| *candidate == declaration)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One fragment color assignment from a visible identifier.
struct FragmentColorSource<'src> {
    /// Fragment color lvalue token index.
    index: usize,
    /// Assigned identifier source span.
    assigned_span: SourceSpan,
    /// Declaration assigned to the fragment color.
    declaration: VectorDeclaration<'src>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One raw fragment color assignment from an identifier.
struct FragmentColorSourceAssignment<'src> {
    /// Fragment color lvalue token index.
    index: usize,
    /// Assigned identifier name.
    name: &'src str,
    /// Assigned identifier source span.
    span: SourceSpan,
}

/// Iterator over raw `gl_FragColor = name` and `_we_FragColor = name`
/// assignments before declaration binding.
struct FragmentColorSourceAssignments<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [Token<'src>],
    /// Next token index to inspect.
    cursor: usize,
}

impl<'tokens, 'src> From<&'tokens [Token<'src>]> for FragmentColorSourceAssignments<'tokens, 'src> {
    fn from(tokens: &'tokens [Token<'src>]) -> Self {
        Self { tokens, cursor: 0 }
    }
}

impl<'src> Iterator for FragmentColorSourceAssignments<'_, 'src> {
    type Item = FragmentColorSourceAssignment<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < self.tokens.len() {
            let index = self.cursor;
            self.cursor += 1;

            let TokenKind::Identifier("gl_FragColor" | "_we_FragColor") = self.tokens[index].kind
            else {
                continue;
            };
            let search = TokenSearch::new(self.tokens);
            let Some(equals) = search.next_non_comment(index + 1) else {
                continue;
            };
            if !matches!(self.tokens[equals].kind, TokenKind::Punctuation('=')) {
                continue;
            }
            let Some(value) = search.next_non_comment(equals + 1) else {
                continue;
            };
            let TokenKind::Identifier(name) = self.tokens[value].kind else {
                continue;
            };
            let Some(after_value) = search.next_non_comment(value + 1) else {
                continue;
            };
            if !matches!(self.tokens[after_value].kind, TokenKind::Semicolon) {
                continue;
            }

            return Some(FragmentColorSourceAssignment {
                index,
                name,
                span: self.tokens[value].span,
            });
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Scoped vec4 declaration.
struct VectorDeclaration<'src> {
    /// Declared name.
    name: &'src str,
    /// First token where the declaration is visible.
    visible_start: usize,
    /// First token outside the declaration's lexical scope.
    scope_end: usize,
}

impl VectorDeclaration<'_> {
    /// Returns whether the declaration is visible at `index`.
    const fn visible_at(self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Parsed alpha member expression.
struct AlphaLvalue {
    /// Last token in the member expression.
    end: usize,
    /// Whether this expression targets the fragment color output.
    frag_color: bool,
}

impl AlphaLvalue {
    /// Parses `name.a` from `index`.
    fn from(tokens: &[Token<'_>], index: usize) -> Option<Self> {
        let search = TokenSearch::new(tokens);
        let TokenKind::Identifier(name) = tokens.get(index)?.kind else {
            return None;
        };
        let dot = search.next_non_comment(index + 1)?;
        if !matches!(tokens[dot].kind, TokenKind::Punctuation('.')) {
            return None;
        }
        let field = search.next_non_comment(dot + 1)?;
        if !matches!(tokens[field].kind, TokenKind::Identifier("a")) {
            return None;
        }
        Some(Self {
            end: field,
            frag_color: matches!(name, "gl_FragColor" | "_we_FragColor"),
        })
    }
}
