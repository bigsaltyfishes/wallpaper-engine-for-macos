//! Alpha-to-coverage derivative idiom codegen.

use linkme::distributed_slice;
use smol_str::SmolStr;

use super::{
    ALPHA_TO_COVERAGE, CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES,
    RESERVED_IDENTIFIERS, StrategyContext,
};
use crate::{
    ShaderResult, ShaderStageKind, SourceSpan,
    codegen::{
        ExpressionReplacement, Fixup, ScopedDeclarationFacts, ScopedDeclarationFactsConfig,
        ScopedDeclarationTypeMode,
    },
    syntax::FunctionCalls,
    tokenizer::{
        AccessOperator, AssignmentOperator, LiteralValue,
        OperatorType::{Access, Assignment},
        TokenCursor, TypedToken, TypedTokenFacts,
    },
};

/// Replaces legacy alpha-to-coverage derivative sharpening with the
/// pre-derivative color alpha.
struct AlphaToCoverageStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static ALPHA_TO_COVERAGE_POLICY: CodegenStrategy = CodegenStrategy {
    name: ALPHA_TO_COVERAGE,
    stage: CodegenStage::SemanticRewrite,
    after: &[RESERVED_IDENTIFIERS],
    emitter: &AlphaToCoverageStrategy,
};

impl Emitable for AlphaToCoverageStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        if context.context().module.stage() != ShaderStageKind::Fragment {
            return Ok(());
        }

        let token_stream = context.context().module.token_stream();
        let token_facts = token_stream.facts();
        let declarations: Vec<VectorDeclaration> = ScopedDeclarationFacts::collect(
            context.context().module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Builtins,
                local_types: ScopedDeclarationTypeMode::Builtins,
            },
        )
        .declarations()
        .iter()
        .filter_map(|declaration| {
            matches!(declaration.ty(), "vec4" | "float4").then_some(VectorDeclaration {
                name: declaration.name().into(),
                visible_start: declaration.visible_start(),
                scope_end: declaration.scope_end(),
            })
        })
        .collect();
        let tokens = token_stream.cursor();
        let assignments = (0..tokens.len())
            .filter_map(|assignment| {
                let target = tokens[assignment].kind().identifier_text()?;
                if !matches!(target, "gl_FragColor" | "_we_FragColor") {
                    return None;
                }
                let equals = tokens.next_non_comment(assignment + 1)?;
                if !matches!(
                    tokens[equals].kind(),
                    TypedToken::Operator(Assignment(AssignmentOperator::Assign))
                ) {
                    return None;
                }
                let value = tokens.next_non_comment(equals + 1)?;
                let TypedToken::Identifier(name) = tokens[value].kind() else {
                    return None;
                };
                let after_value = tokens.next_non_comment(value + 1)?;
                if !matches!(tokens[after_value].kind(), TypedToken::Semicolon) {
                    return None;
                }
                let declaration = declarations
                    .iter()
                    .rev()
                    .find(|declaration| {
                        declaration.name == *name && declaration.visible_at(assignment)
                    })
                    .cloned()?;
                Some(FragmentColorSource {
                    index: assignment,
                    assigned_span: tokens[value].span(),
                    declaration,
                })
            })
            .collect();
        let fragment_color_sources = FragmentColorSources {
            assignments,
            declarations,
        };
        for assignment in (AlphaAssignments {
            tokens: token_stream.cursor(),
            facts: &token_facts,
            next: 0,
        }) {
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

/// Alpha assignment matching the derivative idiom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AlphaAssignment {
    /// Matched RHS span to replace.
    span: SourceSpan,
    /// Whether the matched expression came from a saturate wrapper.
    clamp: bool,
    /// First assignment token.
    start: usize,
}

/// Iterator over matching alpha-to-coverage assignments.
struct AlphaAssignments<'a, 'b> {
    /// Token storage.
    tokens: TokenCursor<'a>,
    /// Tokenizer facts.
    facts: &'b TypedTokenFacts,
    /// Next token index to inspect.
    next: usize,
}

impl Iterator for AlphaAssignments<'_, '_> {
    type Item = AlphaAssignment;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.tokens.len() {
            let index = self.next;
            self.next += 1;
            let tokens = self.tokens;
            let facts = self.facts;
            let Some(lvalue) = AlphaLvalue::parse_at(tokens, index) else {
                continue;
            };
            if !lvalue.frag_color {
                continue;
            }
            let search = tokens;
            let Some(equals) = search.next_non_comment(lvalue.end + 1) else {
                continue;
            };
            if !matches!(
                tokens[equals].kind(),
                TypedToken::Operator(Assignment(AssignmentOperator::Assign))
            ) {
                continue;
            }
            let Some(rhs_start) = search.next_non_comment(equals + 1) else {
                continue;
            };
            let inner = FunctionCalls::new(tokens, facts.calls())
                .find(|call| call.name_index == rhs_start && call.name() == "saturate")
                .and_then(|call| {
                    let argument = call.first_argument()?;
                    let start = argument.start();
                    let end = tokens.previous_non_comment(call.close_index)?;
                    Some((start, end, call.span(), true))
                })
                .or_else(|| {
                    let semicolon = tokens.top_level_semicolon_from(rhs_start)?;
                    let end = tokens.previous_non_comment(semicolon)?;
                    let span =
                        SourceSpan::new(tokens[rhs_start].span().start(), tokens[end].span().end())
                            .ok()?;
                    (rhs_start <= end).then_some((rhs_start, end, span, false))
                });
            let Some((inner_start, inner_end, span, clamp)) = inner else {
                continue;
            };
            let alpha_references = (inner_start..=inner_end)
                .filter(|index| {
                    AlphaLvalue::parse_at(tokens, *index).is_some_and(|lvalue| lvalue.frag_color)
                })
                .count();
            let derivative_idiom = alpha_references >= 2
                && tokens[inner_start..inner_end + 1].iter().any(|token| {
                    matches!(
                        token.kind(),
                        kind if kind.identifier_text().is_some_and(|name| {
                            matches!(name, "fwidth" | "dFdx" | "dFdy" | "ddx" | "ddy")
                        })
                    )
                })
                && tokens[inner_start..inner_end + 1]
                    .iter()
                    .any(|token| token.kind().identifier_text() == Some("max"))
                && tokens[inner_start..inner_end + 1]
                    .iter()
                    .filter(|token| {
                        matches!(
                            token.kind(),
                            TypedToken::Literal(LiteralValue::Number(text)) if text == "0.5"
                        )
                    })
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

/// Visible identifier assignments into the fragment color output.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FragmentColorSources {
    /// Source assignments in token order.
    assignments: Vec<FragmentColorSource>,
    /// Scoped vector declaration facts.
    declarations: Vec<VectorDeclaration>,
}

impl FragmentColorSources {
    /// Returns the nearest visible identifier assigned to the fragment color
    /// before `before`.
    fn visible_source_before(&self, before: usize) -> Option<FragmentColorSource> {
        self.assignments
            .iter()
            .rev()
            .find(|assignment| {
                assignment.index < before
                    && self.visible_declaration(&assignment.declaration, before)
            })
            .cloned()
    }

    /// Returns whether `declaration` is the visible vec4 binding at `index`.
    fn visible_declaration(&self, declaration: &VectorDeclaration, index: usize) -> bool {
        self.declarations
            .iter()
            .rev()
            .find(|candidate| candidate.name == declaration.name && candidate.visible_at(index))
            .is_some_and(|candidate| candidate == declaration)
    }
}

/// One fragment color assignment from a visible identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
struct FragmentColorSource {
    /// Fragment color lvalue token index.
    index: usize,
    /// Assigned identifier source span.
    assigned_span: SourceSpan,
    /// Declaration assigned to the fragment color.
    declaration: VectorDeclaration,
}

/// Scoped vec4 declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
struct VectorDeclaration {
    /// Declared name.
    name: SmolStr,
    /// First token where the declaration is visible.
    visible_start: usize,
    /// First token outside the declaration's lexical scope.
    scope_end: usize,
}

impl VectorDeclaration {
    /// Returns whether the declaration is visible at `index`.
    const fn visible_at(&self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}

/// Parsed alpha member expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AlphaLvalue {
    /// Last token in the member expression.
    end: usize,
    /// Whether this expression targets the fragment color output.
    frag_color: bool,
}

impl AlphaLvalue {
    /// Parses `name.a` from `index`.
    fn parse_at(tokens: TokenCursor<'_>, index: usize) -> Option<Self> {
        let search = tokens;
        let name = tokens.get(index)?.kind().identifier_text()?;
        let dot = search.next_non_comment(index + 1)?;
        if !matches!(
            tokens[dot].kind(),
            TypedToken::Operator(Access(AccessOperator::Member))
        ) {
            return None;
        }
        let field = search.next_non_comment(dot + 1)?;
        if tokens[field].kind().identifier_text() != Some("a") {
            return None;
        }
        Some(Self {
            end: field,
            frag_color: matches!(name, "gl_FragColor" | "_we_FragColor"),
        })
    }
}
