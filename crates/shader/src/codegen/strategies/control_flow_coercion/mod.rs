//! Control-flow scalar coercions accepted by Wallpaper Engine shaders.

use crate::tokenizer::{
    ConditionalOperator, OperatorType::Conditional, TokenIndexRange, TypedToken,
};
/// Array subscript index coercions.
mod array_index;
/// Boolean-to-float declaration and compound-assignment coercions.
mod bool_coercion;
/// Shared expression classifiers.
mod expr;
/// Integer `for` loop bound casts.
mod for_bounds;
/// Integer declarations initialized by float expressions.
mod int_initializer;
/// `int` declarations initialized by `step`.
mod int_step;
/// Float modulo lowering.
mod modulo;
/// Statement-level token ranges.
mod statements;
/// Scoped scalar symbol facts.
mod symbols;

use linkme::distributed_slice;

use self::{
    array_index::FloatArraySubscriptIndex,
    bool_coercion::{BoolFloatInitializer, FloatTimesBool},
    expr::{NumericCondition, NumericConditionInput},
    for_bounds::IntegerForLoopCast,
    int_initializer::IntFloatInitializer,
    int_step::IntStepInitializer,
    modulo::FloatModulo,
    statements::StatementFixupInput,
    symbols::SymbolFacts,
};
use super::{
    ARRAY_PARAMETERS, CONTROL_FLOW_COERCION, CodegenStage, CodegenStrategy, Emitable,
    GENERAL_POLICIES, LEGACY_BUILTINS, RESERVED_IDENTIFIERS, StrategyContext, TEXTURE_SAMPLING,
};
use crate::{
    ShaderResult,
    codegen::{ExpressionReplacement, Fixup},
};

/// Rewrites C++-style scalar control-flow coercions to GLSL expressions.
struct ControlFlowCoercionStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static CONTROL_FLOW_COERCION_POLICY: CodegenStrategy = CodegenStrategy {
    name: CONTROL_FLOW_COERCION,
    stage: CodegenStage::TypeCodegen,
    after: &[
        RESERVED_IDENTIFIERS,
        ARRAY_PARAMETERS,
        TEXTURE_SAMPLING,
        LEGACY_BUILTINS,
    ],
    emitter: &ControlFlowCoercionStrategy,
};

impl Emitable for ControlFlowCoercionStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let module = context.context().module;
        let tokens = module.token_stream().cursor();
        let cursor = tokens;
        let token_facts = module.token_facts().clone();
        let facts = SymbolFacts::new(module);

        for fact in token_facts.statements() {
            let statement = StatementFixupInput { fact };
            for fixup in (FloatModulo {
                statement,
                facts: &facts,
                token_facts: &token_facts,
            })
            .lowering_fixups(cursor)
            {
                context.context().fixups.push(fixup);
            }
            for fixup in (IntStepInitializer {
                statement,
                token_facts: &token_facts,
            })
            .fixups(cursor)
            {
                context.context().fixups.push(fixup);
            }
            for fixup in (IntFloatInitializer {
                statement,
                facts: &facts,
                token_facts: &token_facts,
            })
            .fixups(cursor)
            {
                context.context().fixups.push(fixup);
            }
            for fixup in (BoolFloatInitializer {
                statement,
                facts: &facts,
                token_facts: &token_facts,
            })
            .fixups(cursor)
            {
                context.context().fixups.push(fixup);
            }
            if let Ok(fixup) = (FloatTimesBool {
                statement,
                facts: &facts,
            })
            .build_fixup(cursor)
            {
                context.context().fixups.push(fixup);
            }
            for fixup in statement.numeric_ternary_condition_fixups(cursor, &token_facts, &facts) {
                context.context().fixups.push(fixup);
            }
            for fixup in (FloatArraySubscriptIndex {
                statement,
                facts: &facts,
                token_facts: &token_facts,
            })
            .fixups(cursor)
            {
                context.context().fixups.push(fixup);
            }
        }

        for statement in token_facts.for_loops() {
            for fixup in (IntegerForLoopCast {
                statement: statement.clone(),
            })
            .fixups(cursor)?
            {
                context.context().fixups.push(fixup);
            }
        }

        for condition in token_facts.conditions() {
            let range = condition.range();
            if let Ok(condition) = NumericCondition::extract(
                NumericConditionInput {
                    start: range.start(),
                    end: range.last().expect("condition ranges are non-empty"),
                    facts: &facts,
                    token_facts: &token_facts,
                },
                tokens,
            ) {
                let replacement = condition.replacement();
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(condition.span, replacement));
            }
        }

        Ok(())
    }
}

impl StatementFixupInput<'_> {
    /// Emits all numeric-to-boolean ternary condition fixups in this statement.
    pub(crate) fn numeric_ternary_condition_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
        token_facts: &crate::tokenizer::TypedTokenFacts,
        facts: &SymbolFacts<'_>,
    ) -> Vec<Fixup> {
        let Some(semicolon) = self.semicolon(tokens) else {
            return Vec::new();
        };
        let statement_body = TokenIndexRange::new(self.start(), semicolon);
        let mut cursor = statement_body.start();
        let mut fixups = Vec::new();
        while cursor < statement_body.end() {
            let Some(question) = tokens.next_top_level_operator(
                TokenIndexRange::new(cursor, statement_body.end()),
                Conditional(ConditionalOperator::Question),
            ) else {
                break;
            };

            let mut start = self.start();
            let mut paren_depth = 0usize;
            let mut bracket_depth = 0usize;
            let mut malformed_condition = false;
            for (index, token) in tokens.iter().enumerate().take(question).skip(self.start()) {
                match token.kind() {
                    kind if kind.is_left_paren() => paren_depth += 1,
                    kind if kind.is_right_paren() => {
                        let Some(depth) = paren_depth.checked_sub(1) else {
                            malformed_condition = true;
                            break;
                        };
                        paren_depth = depth;
                    }
                    kind if kind.is_left_square() => bracket_depth += 1,
                    kind if kind.is_right_square() => {
                        let Some(depth) = bracket_depth.checked_sub(1) else {
                            malformed_condition = true;
                            break;
                        };
                        bracket_depth = depth;
                    }
                    TypedToken::Operator(Conditional(
                        ConditionalOperator::Question | ConditionalOperator::Colon,
                    )) if paren_depth == 0 && bracket_depth == 0 => {
                        start = index + 1;
                    }
                    TypedToken::Operator(operator)
                        if paren_depth == 0
                            && bracket_depth == 0
                            && operator.is_simple_assignment() =>
                    {
                        let next_is_equals = matches!(
                            tokens
                                .next_non_comment(index + 1)
                                .map(|next| tokens[next].kind()),
                            Some(kind) if kind.is_simple_assignment_operator()
                        );
                        if !next_is_equals {
                            start = index + 1;
                        }
                    }
                    _ => {}
                }
            }

            let Some(start) = tokens.next_non_comment(start) else {
                cursor = question + 1;
                continue;
            };
            let Some(end) = tokens.previous_non_comment(question) else {
                cursor = question + 1;
                continue;
            };
            cursor = question + 1;
            if malformed_condition || start > end {
                continue;
            }
            if let Ok(condition) = NumericCondition::extract(
                NumericConditionInput {
                    start,
                    end,
                    facts,
                    token_facts,
                },
                tokens,
            ) {
                fixups.push(Fixup::replace(condition.span, condition.replacement()));
            }
        }
        fixups
    }
}

impl NumericCondition {
    /// Builds a numeric condition coercion while preserving child fixups inside
    /// the condition expression.
    fn replacement(self) -> ExpressionReplacement {
        ExpressionReplacement::new()
            .with_source(self.span)
            .with_text(" != ")
            .with_text(self.zero_literal())
    }
}
