use super::{
    Fixup, LocalDeclaration, SourceSpan, StrategyContext, TypedToken, assignment::AssignmentLhs,
    types::VectorTypeBindings,
};
use crate::{
    codegen::expressions::analysis::{VectorExpressionAnalyzer, VectorWidth},
    syntax::CallArgument,
    tokenizer::{
        ArithmeticOperator, AssignmentOperator, OperatorFact,
        OperatorType::{Arithmetic, Assignment},
        TokenCursor, TokenIndexRange, TypedTokenFacts,
    },
};

/// Binary expressions that mix `vec3` identifiers with `vec2` constructors.
#[derive(Default)]
pub(super) struct Vec3Vec2BinaryExpressions {
    /// Constructor operands that need widening.
    pub items: Vec<Vec3Vec2BinaryExpression>,
}

impl Vec3Vec2BinaryExpressions {
    /// Scans tokens for `vec3`/`vec2` binary expressions.
    pub(super) fn collect(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) {
        let analyzer = VectorExpressionAnalyzer { facts, token_facts };
        for fact in token_facts.declarations() {
            let Some(declaration) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            if !matches!(
                VectorWidth::classify_constructor(declaration.ty()),
                Some(VectorWidth::Three)
            ) {
                continue;
            }
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            let Some(expression) =
                token_facts.expression_covering(initializer.start()..initializer.end() + 1)
            else {
                continue;
            };
            self.collect_expression(tokens, analyzer, expression);
        }
        for expression in token_facts.expressions() {
            let Some(equals) = tokens.previous_non_comment(expression.range().start()) else {
                continue;
            };
            if !matches!(
                tokens[equals].kind(),
                TypedToken::Operator(Assignment(AssignmentOperator::Assign))
            ) {
                continue;
            }
            if AssignmentLhs::before_assignment(tokens, equals)
                .and_then(|lhs| lhs.vector_width_with_facts(tokens, token_facts, facts))
                != Some(VectorWidth::Three)
            {
                continue;
            }
            self.collect_expression(tokens, analyzer, expression);
        }
    }

    /// Scans one expression for constructor operands that should be widened.
    fn collect_expression(
        &mut self,
        tokens: TokenCursor<'_>,
        analyzer: VectorExpressionAnalyzer<'_, VectorTypeBindings<'_>>,
        expression: &crate::tokenizer::ExpressionFact,
    ) {
        let operators = expression.matching_top_level_operators(&[
            Arithmetic(ArithmeticOperator::Add),
            Arithmetic(ArithmeticOperator::Subtract),
        ]);
        if operators.is_empty() {
            return;
        }
        let operands = expression.operand_ranges_for(&operators);
        if operands.len() != operators.len() + 1 {
            return;
        }
        for (position, operator) in operators.iter().enumerate() {
            self.collect_operator(tokens, analyzer, *operator, &operands, position);
        }
    }

    /// Records a `vec2` constructor operand next to a `vec3` additive operand.
    fn collect_operator(
        &mut self,
        tokens: TokenCursor<'_>,
        analyzer: VectorExpressionAnalyzer<'_, VectorTypeBindings<'_>>,
        _operator: OperatorFact,
        operands: &[TokenIndexRange],
        position: usize,
    ) {
        let Some(left) = operands.get(position).copied() else {
            return;
        };
        let Some(right) = operands.get(position + 1).copied() else {
            return;
        };
        let left_width = CallArgument::trim_from_bounds(tokens, left.start(), left.end())
            .and_then(|argument| analyzer.argument_vector_width(tokens, argument));
        let right_width = CallArgument::trim_from_bounds(tokens, right.start(), right.end())
            .and_then(|argument| analyzer.argument_vector_width(tokens, argument));
        let expression = if left_width == Some(VectorWidth::Three) {
            VectorExpressionAnalyzer::<VectorTypeBindings<'_>>::constructor_operand_span(
                tokens,
                right.start(),
                VectorWidth::Two,
            )
            .map(|operand| Vec3Vec2BinaryExpression { operand })
        } else if right_width == Some(VectorWidth::Three) {
            VectorExpressionAnalyzer::<VectorTypeBindings<'_>>::constructor_operand_span(
                tokens,
                left.start(),
                VectorWidth::Two,
            )
            .map(|operand| Vec3Vec2BinaryExpression { operand })
        } else {
            None
        };
        if let Some(expression) = expression
            && !self.items.contains(&expression)
        {
            self.items.push(expression);
        }
    }
}
/// One `vec2` operand that needs widening in a binary expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Vec3Vec2BinaryExpression {
    /// Full vec2 constructor span.
    pub operand: SourceSpan,
}

impl Vec3Vec2BinaryExpression {
    /// Emits constructor insertion fixups for this binary operand.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        context.context().fixups.push(Fixup::insert(
            self.operand.start_point(),
            "vec3(".to_owned(),
        ));
        context
            .context()
            .fixups
            .push(Fixup::insert(self.operand.end_point(), ", 0.0)".to_owned()));
    }
}
/// Vector binary expressions whose wider operands need target-width swizzles.
#[derive(Default)]
pub(super) struct VectorBinaryExpressions {
    /// Swizzle insertions in source order.
    pub items: Vec<VectorBinaryExpression>,
}

impl VectorBinaryExpressions {
    /// Scans tokens for vector binary expressions that need swizzles.
    pub(super) fn collect(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) {
        let token_cursor = tokens;
        for fact in token_facts.declarations() {
            let Some(declaration) = LocalDeclaration::from_declaration_fact(token_cursor, fact)
            else {
                continue;
            };
            let Some(target_width) = VectorWidth::classify_constructor(declaration.ty()) else {
                continue;
            };
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            self.items.extend(self.expression_swizzles(
                token_cursor,
                TokenIndexRange::from_inclusive(initializer.start(), initializer.end()),
                target_width,
                token_facts,
                facts,
            ));
        }
        for (index, token) in tokens.iter().enumerate() {
            if !matches!(
                token.kind(),
                TypedToken::Operator(Assignment(AssignmentOperator::Assign))
            ) {
                continue;
            }
            let Some(lhs) = AssignmentLhs::before_assignment(token_cursor, index) else {
                continue;
            };
            let Some(target_width) = lhs.vector_width_with_facts(token_cursor, token_facts, facts)
            else {
                continue;
            };
            let search = tokens;
            let Some(rhs_start) = search.next_non_comment(index + 1) else {
                continue;
            };
            let Some(statement_end) = search.top_level_semicolon_from(rhs_start) else {
                continue;
            };
            let Some(rhs_end) = search.previous_non_comment(statement_end) else {
                continue;
            };
            self.items.extend(self.expression_swizzles(
                token_cursor,
                TokenIndexRange::from_inclusive(rhs_start, rhs_end),
                target_width,
                token_facts,
                facts,
            ));
        }
    }

    /// Returns unique wider operand swizzles for one binary expression range.
    fn expression_swizzles(
        &self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
        target_width: VectorWidth,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) -> Vec<VectorBinaryExpression> {
        VectorExpressionAnalyzer { facts, token_facts }
            .binary_operand_swizzles(tokens, range, target_width)
            .into_iter()
            .map(|swizzle| VectorBinaryExpression {
                insertion: swizzle.insertion,
                swizzle: swizzle.swizzle,
            })
            .filter(|item| !self.items.contains(item))
            .collect()
    }
}
/// One wide vector binary operand that needs a trailing swizzle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VectorBinaryExpression {
    /// Insertion point immediately after the operand.
    pub insertion: SourceSpan,
    /// Swizzle text to insert.
    pub swizzle: &'static str,
}

impl VectorBinaryExpression {
    /// Emits the operand swizzle insertion.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, self.swizzle.to_owned()));
    }
}
