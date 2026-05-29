use smol_str::SmolStr;

use super::{
    Fixup, SourceSpan, StrategyContext, TypedToken, assignment::AssignmentLhs,
    initializer::FloatLiteral, types::VectorTypeBindings,
};
use crate::{
    codegen::expressions::analysis::{VectorExpressionAnalyzer, VectorWidth},
    syntax::{CallArgument, CallArguments, FunctionCall},
    tokenizer::{
        AccessOperator, AssignmentOperator, LiteralValue,
        OperatorType::{Access, Assignment},
        TokenCursor, TypedTokenFacts,
    },
};

/// Builtin function whose scalar arguments are coerced in legacy shaders.
#[derive(Clone)]
pub(super) struct FunctionCoercion<'src> {
    /// Original function call.
    pub call: FunctionCall,
    /// Coercion rules for this function.
    pub function: CoercionFunction,
    /// Known vector declarations.
    pub vector_facts: &'src VectorTypeBindings<'src>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'src TypedTokenFacts,
}

impl FunctionCoercion<'_> {
    /// Returns the width implied by the assignment target of this call.
    fn context_width(&self, tokens: TokenCursor<'_>) -> Option<VectorWidth> {
        let search = tokens;
        if !self.call.has_assignment_context_tail(tokens) {
            return None;
        }
        let equals = search.previous_non_comment(self.call.name_index)?;
        if !matches!(
            tokens[equals].kind(),
            TypedToken::Operator(Assignment(AssignmentOperator::Assign))
        ) {
            return None;
        }
        AssignmentLhs::before_assignment(tokens, equals)?.vector_width_with_facts(
            tokens,
            self.token_facts,
            self.vector_facts,
        )
    }

    /// Returns the vector width that scalar arguments should broadcast to.
    fn scalar_broadcast_width(
        &self,
        tokens: TokenCursor<'_>,
        arguments: &CallArguments,
    ) -> Option<VectorWidth> {
        if !self.function.broadcasts() {
            return None;
        }
        if let Some(context_width) = self.context_width(tokens) {
            let analyzer = VectorExpressionAnalyzer {
                facts: self.vector_facts,
                token_facts: self.token_facts,
            };
            return arguments
                .iter()
                .filter_map(|argument| analyzer.argument_vector_width(tokens, argument))
                .all(|argument_width| argument_width >= context_width)
                .then_some(context_width);
        }

        arguments.coercion_width(tokens, self.token_facts, self.vector_facts, self.function)
    }

    /// Emits numeric literal and scalar broadcast fixups for this call.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>, tokens: TokenCursor<'_>) {
        let arguments = &self.call.arguments;
        let width = self.scalar_broadcast_width(tokens, arguments);

        for (index, argument) in arguments.iter().enumerate() {
            if let Ok(literal) = NumericLiteral::parse_argument(tokens, argument)
                && (self.function.promotes_integer_literals() || width.is_some())
            {
                literal.emit_float(context, self.function.broadcast_width(width, index));
            }
        }

        let Some(width) = width else {
            return;
        };

        let analyzer = VectorExpressionAnalyzer {
            facts: self.vector_facts,
            token_facts: self.token_facts,
        };
        for (index, argument) in arguments.iter().enumerate() {
            let Some(width) = self.function.broadcast_width(Some(width), index) else {
                continue;
            };
            if analyzer.argument_vector_width(tokens, argument).is_some()
                || !analyzer.argument_is_scalar_like(tokens, argument)
            {
                continue;
            }
            if NumericLiteral::parse_argument(tokens, argument).is_ok() {
                continue;
            }
            context.context().fixups.push(Fixup::insert(
                argument.span().start_point(),
                format!("{}(", width.constructor()),
            ));
            context
                .context()
                .fixups
                .push(Fixup::insert(argument.span().end_point(), ")".to_owned()));
        }

        if !self.function.narrows_vector_arguments() {
            return;
        }
        for argument in arguments.iter() {
            if analyzer
                .argument_vector_width(tokens, argument)
                .is_none_or(|argument_width| argument_width <= width)
            {
                continue;
            }
            let Some(swizzle) = width.narrow_swizzle() else {
                continue;
            };
            let suffix =
                if VectorExpressionAnalyzer::<VectorTypeBindings<'_>>::needs_parentheses_for_swizzle(
                    argument,
                ) {
                    context
                        .context()
                        .fixups
                        .push(Fixup::insert(argument.span().start_point(), "(".to_owned()));
                    format!("){swizzle}")
                } else {
                    swizzle.to_owned()
                };
            context
                .context()
                .fixups
                .push(Fixup::insert(argument.span().end_point(), suffix));
        }
    }
}

impl FunctionCall {
    /// Returns whether the call is not immediately followed by member or index
    /// access that should own the assignment context instead.
    fn has_assignment_context_tail(&self, tokens: TokenCursor<'_>) -> bool {
        tokens
            .next_non_comment(self.close_index + 1)
            .is_none_or(|next| {
                !matches!(
                    tokens[next].kind(),
                    TypedToken::Operator(Access(AccessOperator::Member))
                ) && !tokens[next].kind().is_left_square()
            })
    }
}
/// Type coercion behavior for one builtin name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CoercionFunction {
    /// `mix`.
    Mix,
    /// `smoothstep`.
    Smoothstep,
    /// `step`.
    Step,
    /// `pow`.
    Pow,
    /// `clamp`.
    Clamp,
    /// `min`.
    Min,
    /// `max`.
    Max,
}

impl CoercionFunction {
    /// Returns whether integer literals are promoted to float literals.
    pub(super) const fn promotes_integer_literals(self) -> bool {
        matches!(
            self,
            Self::Mix | Self::Smoothstep | Self::Step | Self::Pow | Self::Clamp
        )
    }

    /// Returns whether scalar arguments should be broadcast from peer vector
    /// width.
    pub(super) const fn broadcasts(self) -> bool {
        matches!(
            self,
            Self::Mix
                | Self::Smoothstep
                | Self::Step
                | Self::Pow
                | Self::Clamp
                | Self::Min
                | Self::Max
        )
    }

    /// Returns the width used for a scalar argument at `index`.
    pub(super) const fn broadcast_width(
        self,
        width: Option<VectorWidth>,
        index: usize,
    ) -> Option<VectorWidth> {
        match self {
            Self::Mix if index == 2 => None,
            Self::Step if index == 0 => None,
            _ => width,
        }
    }

    /// Returns whether a vector argument at `index` participates in width
    /// selection.
    pub(super) const fn selects_vector_width(self, index: usize) -> bool {
        match self {
            Self::Mix if index == 2 => false,
            Self::Step if index == 0 => false,
            _ => true,
        }
    }

    /// Returns whether vector arguments should be narrowed to the selected
    /// width.
    pub(super) const fn narrows_vector_arguments(self) -> bool {
        matches!(
            self,
            Self::Mix
                | Self::Smoothstep
                | Self::Step
                | Self::Pow
                | Self::Clamp
                | Self::Min
                | Self::Max
        )
    }
}
/// Argument-list width selection for builtin coercion functions.
pub(super) trait CoercionArguments {
    /// Returns the vector width that builtin arguments should conform to.
    fn coercion_width(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
        function: CoercionFunction,
    ) -> Option<VectorWidth>;
}

impl CoercionArguments for CallArguments {
    fn coercion_width(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
        function: CoercionFunction,
    ) -> Option<VectorWidth> {
        let analyzer = VectorExpressionAnalyzer { facts, token_facts };
        let primary_argument_width = self
            .iter()
            .enumerate()
            .filter(|(index, _argument)| function.selects_vector_width(*index))
            .filter_map(|(_index, argument)| analyzer.argument_vector_width(tokens, argument))
            .min();
        let special_argument_width = self
            .iter()
            .enumerate()
            .filter(|(index, _argument)| !function.selects_vector_width(*index))
            .filter_map(|(_index, argument)| analyzer.argument_vector_width(tokens, argument))
            .min();
        match (primary_argument_width, special_argument_width) {
            (Some(primary), Some(special)) => Some(primary.min(special)),
            (Some(width), None) | (None, Some(width)) => Some(width),
            (None, None) => None,
        }
    }
}
/// Parsed numeric literal argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NumericLiteral {
    /// Literal source text.
    pub text: SmolStr,
    /// Source span of the literal.
    pub span: SourceSpan,
}

impl NumericLiteral {
    /// Parses an integer literal argument.
    pub(super) fn parse_argument(
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Result<Self, ()> {
        if !argument.is_single_token() {
            return Err(());
        }
        let TypedToken::Literal(LiteralValue::Number(text)) = tokens[argument.start()].kind()
        else {
            return Err(());
        };
        if text
            .bytes()
            .any(|byte| matches!(byte, b'.' | b'e' | b'E' | b'f' | b'F'))
        {
            return Err(());
        }
        Ok(Self {
            text: text.clone(),
            span: tokens[argument.start()].span(),
        })
    }

    /// Emits a float-literal replacement, optionally wrapped in a vector
    /// constructor.
    pub(super) fn emit_float(
        self,
        context: &mut StrategyContext<'_, '_, '_>,
        width: Option<VectorWidth>,
    ) {
        let value = FloatLiteral {
            text: self.text.as_str(),
        }
        .to_string();
        let replacement = width.map_or(value.clone(), |width| {
            format!("{}({value})", width.constructor())
        });
        context
            .context()
            .fixups
            .push(Fixup::replace(self.span, replacement));
    }
}
