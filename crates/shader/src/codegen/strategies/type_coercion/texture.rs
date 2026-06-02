use super::{
    Fixup, SourceSpan, StrategyContext, TextureSamplingCall, TypedToken, types::VectorTypeBindings,
};
use crate::{
    codegen::{
        ExpressionReplacement,
        expressions::analysis::{VectorExpressionAnalyzer, VectorWidth},
    },
    syntax::CallArgument,
    tokenizer::{
        ArithmeticOperator, AssignmentOperator,
        OperatorType::{Arithmetic, Assignment},
    },
};

/// Vector declarations initialized from wider texture samples.
pub(super) struct TextureVectorInitializers {
    /// Narrowing swizzle insertions in source order.
    pub items: Vec<TextureVectorInitializer>,
}

impl TextureVectorInitializers {
    /// Collects texture sample initializers from strategy context.
    pub(super) fn collect_from_context(&mut self, context: &mut StrategyContext<'_, '_, '_>) {
        let state = context.context();
        let tokens = state.module.token_stream().cursor();
        for call in state.module.function_calls() {
            let Some(_texture_call) =
                TextureSamplingCall::classify_call(tokens, &call, &state.declarations)
            else {
                continue;
            };
            if call.has_trailing_swizzle() {
                continue;
            }
            let search = tokens;
            let Some(equals) = search.previous_non_comment(call.name_index) else {
                continue;
            };
            if !matches!(
                tokens[equals].kind(),
                TypedToken::Operator(Assignment(AssignmentOperator::Assign))
            ) {
                continue;
            }
            let Some(name) = search.previous_non_comment(equals) else {
                continue;
            };
            if !matches!(tokens[name].kind(), TypedToken::Identifier(_)) {
                continue;
            }
            let Some(ty) = search.previous_non_comment(name) else {
                continue;
            };
            if let Some(type_name) = tokens[ty].kind().source_text()
                && let Some(width) = VectorWidth::classify_constructor(type_name)
                && width.narrow_swizzle().is_some()
            {
                self.items.push(TextureVectorInitializer {
                    call_span: call.span(),
                    width,
                });
            }
        }
    }
}

impl IntoIterator for TextureVectorInitializers {
    type Item = TextureVectorInitializer;
    type IntoIter = std::vec::IntoIter<TextureVectorInitializer>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
/// Texture sampling calls whose coordinates must match the sampler
/// dimensionality.
pub(super) struct TextureCoordinateArguments {
    /// Coordinate narrowing plans in source order.
    pub items: Vec<TextureCoordinateArgument>,
}

impl TextureCoordinateArguments {
    /// Collects texture coordinate arguments from strategy context.
    pub(super) fn collect_from_context(
        &mut self,
        context: &mut StrategyContext<'_, '_, '_>,
        vector_facts: &VectorTypeBindings<'_>,
    ) {
        let state = context.context();
        let tokens = state.module.token_stream().cursor();
        for call in state.module.function_calls() {
            let Some(_texture_call) =
                TextureSamplingCall::classify_call(tokens, &call, &state.declarations)
            else {
                continue;
            };
            let Some(coordinate) = call.arguments.get(1) else {
                continue;
            };
            let analyzer = VectorExpressionAnalyzer {
                facts: vector_facts,
                token_facts: state.module.token_facts(),
            };
            let mut coordinate_without_wrappers = coordinate;
            while let Some(parenthesized) = CallArgument::trim_from_bounds(
                tokens,
                coordinate_without_wrappers.start() + 1,
                coordinate_without_wrappers.end(),
            )
            .filter(|_| {
                tokens[coordinate_without_wrappers.start()]
                    .kind()
                    .is_left_paren()
                    && tokens[coordinate_without_wrappers.end()]
                        .kind()
                        .is_right_paren()
                    && tokens.matching_right_paren(coordinate_without_wrappers.start())
                        == Some(coordinate_without_wrappers.end())
            }) {
                coordinate_without_wrappers = parenthesized;
            }
            let coordinate_expression = state.module.token_facts().expression_covering(
                coordinate_without_wrappers.start()..coordinate_without_wrappers.end() + 1,
            );
            if coordinate_expression.is_some_and(|expression| {
                !expression
                    .matching_top_level_operators(&[
                        Arithmetic(ArithmeticOperator::Add),
                        Arithmetic(ArithmeticOperator::Subtract),
                        Arithmetic(ArithmeticOperator::Multiply),
                        Arithmetic(ArithmeticOperator::Divide),
                        Arithmetic(ArithmeticOperator::Remainder),
                    ])
                    .is_empty()
            }) {
                self.items.extend(
                    analyzer
                        .sampler2d_coordinate_operands(tokens, coordinate)
                        .into_iter()
                        .map(|operand| TextureCoordinateArgument {
                            span: operand.span,
                            required_width: TextureCoordinateWidth::Sampler2D,
                            expression_wrap: operand.wrap,
                        }),
                );
                continue;
            }
            match analyzer.argument_vector_width(tokens, coordinate) {
                Some(VectorWidth::Three | VectorWidth::Four) => {
                    self.items.push(TextureCoordinateArgument {
                        span: coordinate.span(),
                        required_width: TextureCoordinateWidth::Sampler2D,
                        expression_wrap: VectorExpressionAnalyzer::<VectorTypeBindings<'_>>::needs_parentheses_for_swizzle(coordinate),
                    });
                }
                Some(VectorWidth::Two) => {}
                None => {
                    self.items.extend(
                        analyzer
                            .sampler2d_coordinate_operands(tokens, coordinate)
                            .into_iter()
                            .map(|operand| TextureCoordinateArgument {
                                span: operand.span,
                                required_width: TextureCoordinateWidth::Sampler2D,
                                expression_wrap: operand.wrap,
                            }),
                    );
                }
            }
        }
    }
}
/// Coordinate argument for one texture sampling call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TextureCoordinateArgument {
    /// Source span covering the coordinate expression.
    pub span: SourceSpan,
    /// Texture coordinate dimensionality required by the sampler.
    pub required_width: TextureCoordinateWidth,
    /// Whether the coordinate must be wrapped so the swizzle selects the whole
    /// expression.
    pub expression_wrap: bool,
}

impl TextureCoordinateArgument {
    /// Emits the required coordinate-width narrowing.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        if self.expression_wrap {
            let replacement = ExpressionReplacement::new()
                .with_text("(")
                .with_source(self.span)
                .with_text(format!("){}", self.required_width.swizzle()));
            context
                .context()
                .fixups
                .push(Fixup::replace(self.span, replacement));
        } else {
            context.context().fixups.push(Fixup::insert(
                self.span.end_point(),
                self.required_width.swizzle().to_owned(),
            ));
        }
    }
}
/// Coordinate dimensionality required by a sampler declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextureCoordinateWidth {
    /// `sampler2D` accepts `vec2` coordinates.
    Sampler2D,
}

impl TextureCoordinateWidth {
    /// Returns the swizzle suffix for this coordinate width.
    pub(super) const fn swizzle(self) -> &'static str {
        match self {
            Self::Sampler2D => ".xy",
        }
    }
}
/// Texture initializer that needs a vector-width swizzle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TextureVectorInitializer {
    /// Full texture sampling call span.
    pub call_span: SourceSpan,
    /// Target vector width.
    pub width: VectorWidth,
}

impl TextureVectorInitializer {
    /// Emits insertions around the texture call and a target-width swizzle.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        let Some(swizzle) = self.width.narrow_swizzle() else {
            return;
        };
        context
            .context()
            .fixups
            .push(Fixup::insert(self.call_span.start_point(), "(".to_owned()));
        context.context().fixups.push(Fixup::insert(
            self.call_span.end_point(),
            format!("){swizzle}"),
        ));
    }
}
