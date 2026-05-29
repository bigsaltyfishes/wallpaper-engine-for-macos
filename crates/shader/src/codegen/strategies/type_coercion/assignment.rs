use super::{
    Fixup, SourceSpan, StrategyContext, TypedToken,
    types::{BindingType, VectorTypeBindings},
};
use crate::{
    codegen::{
        ExpressionReplacement,
        declarations::LegacyTypeName,
        expressions::analysis::{SwizzleField, VectorExpressionAnalyzer, VectorWidth},
    },
    syntax::CallArgument,
    tokenizer::{TokenCursor, TokenIndexRange, TypedTokenFacts},
};

/// Assignments whose right-hand side must match a narrowed vector target.
pub(super) struct NarrowVectorAssignments {
    /// Assignment RHS swizzle insertions in source order.
    pub items: Vec<NarrowVectorAssignment>,
}

impl NarrowVectorAssignments {
    /// Collects narrowed vector assignment fixups from strategy context.
    pub(super) fn collect_from_context(
        &mut self,
        context: &mut StrategyContext<'_, '_, '_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) {
        let state = context.context();
        let tokens = state.module.token_stream().cursor();
        for (index, token) in tokens.iter().enumerate() {
            if !token.kind().is_simple_assignment_operator() {
                continue;
            }

            let search = tokens;
            let Some(lhs_end) = search.previous_non_comment(index) else {
                continue;
            };
            if matches!(
                tokens[lhs_end].kind(),
                TypedToken::Operator(operator) if operator.is_assignment()
            ) || search
                .next_non_comment(index + 1)
                .is_none_or(|rhs_start| tokens[rhs_start].kind().is_simple_assignment_operator())
            {
                continue;
            }

            let Some(lhs) = AssignmentLhs::before_assignment(tokens, index) else {
                continue;
            };
            let lhs_width = if let TypedToken::Identifier(name) = tokens[lhs.end()].kind() {
                if let Some(ty) = state.declarations.stage_interface_ty(name) {
                    VectorWidth::classify_constructor(LegacyTypeName::new(ty).glsl())
                } else {
                    lhs.vector_width_with_facts(tokens, token_facts, facts)
                }
            } else {
                lhs.vector_width_with_facts(tokens, token_facts, facts)
            };
            let Some(lhs_width) = lhs_width else {
                continue;
            };

            let Some(rhs_start) = search.next_non_comment(index + 1) else {
                continue;
            };
            let Some(statement_end) = search.top_level_semicolon_from(rhs_start) else {
                continue;
            };
            let Some(rhs_end) = search.previous_non_comment(statement_end) else {
                continue;
            };
            if rhs_start > rhs_end {
                continue;
            }
            let rhs = TokenIndexRange::from_inclusive(rhs_start, rhs_end);
            let Some(rhs_end) = rhs.last() else {
                continue;
            };
            let rhs_width = if let Some(width) =
                VectorExpressionAnalyzer::<VectorTypeBindings<'_>>::terminal_swizzle(tokens, rhs)
                    .map(|swizzle| swizzle.width)
            {
                Some(width)
            } else if let TypedToken::Identifier(name) = tokens[rhs.start()].kind() {
                VectorWidth::classify_constructor(name).and_then(|width| {
                    let open = tokens.next_non_comment(rhs.start() + 1)?;
                    (tokens[open].kind().is_left_paren() && tokens[rhs_end].kind().is_right_paren())
                        .then_some(width)
                })
            } else {
                None
            };
            if rhs_width.is_none_or(|rhs_width| rhs_width <= lhs_width) {
                continue;
            }
            let Some(swizzle) = lhs_width.narrow_swizzle() else {
                continue;
            };
            self.items.push(NarrowVectorAssignment {
                insertion: tokens[rhs_end].span().end_point(),
                swizzle,
            });
        }
    }
}

/// One assignment RHS that needs a trailing narrow swizzle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NarrowVectorAssignment {
    /// Source span immediately after the RHS expression.
    pub insertion: SourceSpan,
    /// Swizzle text to insert.
    pub swizzle: &'static str,
}

impl NarrowVectorAssignment {
    /// Emits the RHS narrowing swizzle insertion.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, self.swizzle.to_owned()));
    }
}

/// Assignments whose scalar RHS must be broadcast to a vector target.
pub(super) struct ScalarVectorAssignments {
    /// Assignment RHS constructor insertions in source order.
    pub items: Vec<ScalarVectorAssignment>,
}

impl ScalarVectorAssignments {
    /// Scans compound assignments that broadcast scalar RHS expressions.
    pub(super) fn collect(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) {
        for (index, token) in tokens.iter().enumerate() {
            if !token.kind().is_compound_assignment_operator() {
                continue;
            }
            let search = tokens;
            let Some(lhs) = AssignmentLhs::before_assignment(tokens, index) else {
                continue;
            };
            let Some(width) = lhs.vector_width_with_facts(tokens, token_facts, facts) else {
                continue;
            };
            let Some(rhs_start) = search.next_non_comment(index + 1) else {
                continue;
            };
            if tokens[rhs_start].kind().is_simple_assignment_operator() {
                continue;
            }
            let Some(statement_end) = search.top_level_semicolon_from(rhs_start) else {
                continue;
            };
            let Some(rhs_end) = search.previous_non_comment(statement_end) else {
                continue;
            };
            if rhs_start > rhs_end {
                continue;
            }
            let range = TokenIndexRange::from_inclusive(rhs_start, rhs_end);
            if !CallArgument::trim_from_bounds(tokens, range.start(), range.end()).is_some_and(
                |argument| {
                    VectorExpressionAnalyzer { facts, token_facts }
                        .argument_is_scalar_like(tokens, argument)
                },
            ) {
                continue;
            }
            let Ok(span) = SourceSpan::new(
                tokens[rhs_start].span().start(),
                tokens[rhs_end].span().end(),
            ) else {
                continue;
            };
            self.items.push(ScalarVectorAssignment { span, width });
        }
    }
}

/// One scalar RHS that needs vector constructor broadcasting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarVectorAssignment {
    /// RHS expression span.
    span: SourceSpan,
    /// Constructor width to emit.
    width: VectorWidth,
}

impl ScalarVectorAssignment {
    /// Emits a constructor replacement around the RHS, preserving nested
    /// expression fixups.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        let replacement = ExpressionReplacement::new()
            .with_text(format!("{}(", self.width.constructor()))
            .with_source(self.span)
            .with_text(")");
        context
            .context()
            .fixups
            .push(Fixup::replace(self.span, replacement));
    }
}

/// Parsed left-hand side facts for an assignment expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AssignmentLhs {
    /// Final non-comment token in the left-hand side expression.
    end: usize,
}

impl AssignmentLhs {
    /// Creates left-hand side facts from a known assignment token.
    pub(super) fn before_assignment(tokens: TokenCursor<'_>, equals: usize) -> Option<Self> {
        let end = tokens.previous_non_comment(equals)?;
        (!tokens[end].kind().is_simple_assignment_operator()).then_some(Self { end })
    }

    /// Returns the final non-comment token in the left-hand side expression.
    pub(super) const fn end(self) -> usize {
        self.end
    }

    /// Resolves vector width with tokenizer declaration facts when available.
    pub(super) fn vector_width_with_facts(
        self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) -> Option<VectorWidth> {
        let TypedToken::Identifier(field) = tokens[self.end].kind() else {
            return None;
        };
        let dot = tokens.previous_non_comment(self.end)?;
        if tokens[dot].kind().is_member_access_operator() {
            let base = tokens.previous_non_comment(dot)?;
            if let Ok(field) = SwizzleField::parse(field)
                && !tokens.previous_non_comment(base).is_some_and(|previous| {
                    matches!(
                        tokens[previous].kind(),
                        kind if kind.is_member_access_operator()
                    )
                })
                && matches!(
                    tokens[base].kind(),
                    TypedToken::Identifier(base_name)
                        if matches!(
                            facts.lookup(base_name, base),
                            Some(BindingType::Vector(_))
                        )
                )
            {
                return Some(field.width);
            }
            return None;
        }
        for declaration in token_facts.declarations() {
            if declaration.name_index() > self.end {
                break;
            }
            if declaration.name_index() == self.end && declaration.name() == field {
                return VectorWidth::classify_constructor(declaration.ty().as_str());
            }
        }
        if let Some(BindingType::Vector(width)) = facts.lookup(field, self.end) {
            return Some(width);
        }
        None
    }
}
