use super::{ExpressionReplacement, SourceSpan, SymbolFacts, SymbolType};
use crate::{
    codegen::{
        DeclaratorInitializer,
        expressions::analysis::{ScalarExpressionAnalyzer, ScalarExpressionFlavor},
    },
    tokenizer::{
        ArithmeticOperator, OperatorFact, OperatorType::Arithmetic, TokenCursor, TokenIndexRange,
        TypedTokenFacts,
    },
};

/// Semantic lowering context for `%` expressions.
#[derive(Clone, Copy)]
pub(super) struct ModuloLowerer<'facts, 'src> {
    /// Known symbol facts.
    pub facts: &'facts SymbolFacts<'src>,
    /// Cached tokenizer facts for call lookups.
    pub token_facts: &'facts TypedTokenFacts,
    /// Modulo lowering style.
    pub mode: ModuloLoweringMode,
}

impl ModuloLowerer<'_, '_> {
    /// Lowers all `%` operators inside this initializer.
    pub(super) fn lower_initializer(
        self,
        tokens: TokenCursor<'_>,
        initializer: DeclaratorInitializer,
    ) -> Result<ExpressionReplacement, ()> {
        self.lower(
            tokens,
            TokenIndexRange::from_inclusive(initializer.start(), initializer.end()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        codegen::expressions::ExpressionRenderer, syntax::ShaderSourceText, tokenizer::TokenStream,
    };

    fn lower_initializer(source: &str) -> String {
        let stream = TokenStream::lex(source).expect("source tokenizes");
        let tokens = stream.cursor();
        let token_facts = stream.facts();
        let facts = SymbolFacts::default();
        let lowered = ModuloLowerer {
            facts: &facts,
            token_facts: &token_facts,
            mode: ModuloLoweringMode::BuiltinFmod,
        }
        .lower(tokens, TokenIndexRange::from_inclusive(0, stream.len() - 1))
        .expect("initializer lowers");

        ExpressionRenderer {
            source: ShaderSourceText::new(source),
            fixups: &[],
        }
        .render_replacement(&lowered, 0)
        .expect("replacement renders")
    }

    #[test]
    fn comparison_boundaries_do_not_swallow_modulo_operands() {
        assert_eq!(lower_initializer("a % b <= c"), "fmod(a, b) <= c");
        assert_eq!(lower_initializer("a % b >= c"), "fmod(a, b) >= c");
        assert_eq!(lower_initializer("a % b == c"), "fmod(a, b) == c");
        assert_eq!(lower_initializer("a % b != c"), "fmod(a, b) != c");
    }
}

/// Available `%` lowering forms.
#[derive(Clone, Copy)]
pub(super) enum ModuloLoweringMode {
    /// Emits `fmod(left, right)`.
    BuiltinFmod,
    /// Emits arithmetic GLSL accepted by Naga's parser.
    NagaCompatible,
}

impl ModuloLowerer<'_, '_> {
    /// Lowers `%` operators while preserving source outside affected segments.
    pub(super) fn lower(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Result<ExpressionReplacement, ()> {
        let start = range.start();
        let end = range.last().ok_or(())?;
        let mut output = ExpressionReplacement::new();
        let range_span = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end())
            .map_err(|_error| ())?;
        let mut copied = range_span.start();

        let expression = self
            .token_facts
            .expression_covering(start..end + 1)
            .ok_or(())?;
        let segments = expression.modulo_lowering_segments();
        if segments.is_empty() {
            self.append_segment(start, end, &mut copied, &mut output, tokens)?;
        } else {
            for segment in segments {
                self.append_segment(
                    segment.start(),
                    segment.end().saturating_sub(1),
                    &mut copied,
                    &mut output,
                    tokens,
                )?;
            }
        }
        output.push_source(SourceSpan::new(copied, range_span.end()).map_err(|_error| ())?);
        Ok(output)
    }

    /// Lowers `%` operators inside nested balanced delimiters only.
    pub(super) fn lower_nested(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Result<ExpressionReplacement, ()> {
        let start = range.start();
        let end = range.last().ok_or(())?;
        let range_span = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end())
            .map_err(|_error| ())?;
        let mut output = ExpressionReplacement::new();
        let mut copied = range_span.start();
        let mut index = start;
        while index <= end {
            let texture_call_open = tokens[index].kind().is_left_paren()
                && tokens.previous_non_comment(index).is_some_and(|previous| {
                    tokens[previous]
                        .kind()
                        .identifier_text()
                        .is_some_and(|name| {
                            matches!(
                                name,
                                "texture"
                                    | "texture2D"
                                    | "tex2D"
                                    | "texSample2D"
                                    | "textureLod"
                                    | "texture2DLod"
                                    | "tex2DLod"
                                    | "texSample2DLod"
                            )
                        })
                });
            if texture_call_open {
                index += 1;
                continue;
            }
            let close = match tokens[index].kind() {
                kind if kind.is_left_paren() => tokens.matching_right_paren(index),
                _ => None,
            };
            let Some(close) = close.filter(|close| *close < range.end()) else {
                index += 1;
                continue;
            };
            output.push_source(
                SourceSpan::new(copied, tokens[index].span().end()).map_err(|_error| ())?,
            );
            let inner_start = tokens.next_non_comment(index + 1);
            let inner_end = tokens.previous_non_comment(close);
            if let (Some(inner_start), Some(inner_end)) = (inner_start, inner_end)
                && inner_start <= inner_end
            {
                output.push_replacement(self.lower(
                    tokens,
                    TokenIndexRange::from_inclusive(inner_start, inner_end),
                )?);
            }
            copied = tokens[close].span().start();
            index = close + 1;
        }
        output.push_source(SourceSpan::new(copied, range_span.end()).map_err(|_error| ())?);
        Ok(output)
    }

    /// Appends this segment, lowering `%` left-associatively.
    pub(super) fn append_segment(
        self,
        segment_start: usize,
        segment_end: usize,
        copied: &mut usize,
        output: &mut ExpressionReplacement,
        tokens: TokenCursor<'_>,
    ) -> Result<(), ()> {
        let Some(start) = tokens.next_non_comment(segment_start) else {
            return Ok(());
        };
        if start > segment_end {
            return Ok(());
        }
        let end = tokens.previous_non_comment(segment_end + 1).ok_or(())?;
        let segment_span = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end())
            .map_err(|_error| ())?;
        output.push_source(SourceSpan::new(*copied, segment_span.start()).map_err(|_error| ())?);
        output.push_replacement(self.lower_segment(tokens, start, end)?);
        *copied = segment_span.end();
        Ok(())
    }

    /// Lowers `%` operators inside this multiplicative segment.
    pub(super) fn lower_segment(
        self,
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
    ) -> Result<ExpressionReplacement, ()> {
        let expression = self
            .token_facts
            .expression_covering(start..end + 1)
            .ok_or(())?;
        let operators = expression.matching_top_level_operators(&[
            Arithmetic(ArithmeticOperator::Multiply),
            Arithmetic(ArithmeticOperator::Divide),
            Arithmetic(ArithmeticOperator::Remainder),
        ]);
        if !operators.iter().any(|operator| {
            matches!(
                operator.operator(),
                Arithmetic(ArithmeticOperator::Remainder)
            )
        }) {
            return self.lower_nested(tokens, TokenIndexRange::from_inclusive(start, end));
        }

        let operands = self.segment_operand_ranges(tokens, start, end, &operators)?;
        let first_operand = operands.first().ok_or(())?;
        let mut acc = self.lower_nested(tokens, *first_operand)?;
        let mut acc_ty = self.expression_type_before(tokens, *first_operand);
        for (position, operator) in operators.iter().enumerate() {
            let right_range = *operands.get(position + 1).ok_or(())?;
            let right = self.lower_nested(tokens, right_range)?;
            let right_ty = self.expression_type_before(tokens, right_range);
            match operator.operator() {
                Arithmetic(ArithmeticOperator::Remainder)
                    if SymbolType::integer_modulo_operands(acc_ty, right_ty) =>
                {
                    acc = ExpressionReplacement::binary(acc, " % ", right);
                    acc_ty = Some(SymbolType::integer_result(acc_ty, right_ty));
                }
                Arithmetic(ArithmeticOperator::Remainder) => {
                    acc = self.float_modulo(acc, right, acc_ty, right_ty);
                    acc_ty = Some(SymbolType::Float);
                }
                operator if operator.is_scalar_binary() => {
                    acc =
                        ExpressionReplacement::binary(acc, format!(" {} ", operator.text()), right);
                    acc_ty = if matches!(acc_ty, Some(SymbolType::Float))
                        || matches!(right_ty, Some(SymbolType::Float))
                    {
                        Some(SymbolType::Float)
                    } else if SymbolType::integer_modulo_operands(acc_ty, right_ty) {
                        Some(SymbolType::integer_result(acc_ty, right_ty))
                    } else {
                        None
                    };
                }
                _ => return Err(()),
            }
        }
        Ok(acc)
    }

    /// Returns the known scalar type for a half-open operand token range.
    pub(super) fn expression_type_before(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Option<SymbolType> {
        ScalarExpressionAnalyzer {
            facts: self.facts,
            token_facts: self.token_facts,
            flavor: ScalarExpressionFlavor::FloatModulo,
        }
        .range_type(tokens, range.start(), range.end().saturating_sub(1))
    }

    /// Returns tokenizer-provided operand ranges for this multiplicative
    /// segment.
    fn segment_operand_ranges(
        self,
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
        operators: &[OperatorFact],
    ) -> Result<Vec<TokenIndexRange>, ()> {
        let expression = self
            .token_facts
            .expression_covering(start..end + 1)
            .ok_or(())?;
        let ranges = expression.operand_ranges_for(operators);
        if ranges.len() == operators.len() + 1
            && ranges.iter().all(|range| {
                tokens
                    .non_comment_range(range.start(), range.end())
                    .is_some_and(|(start, end)| start == range.start() && end + 1 == range.end())
            })
        {
            Ok(ranges)
        } else {
            Err(())
        }
    }

    /// Emits one non-integer modulo expression.
    pub(super) fn float_modulo(
        self,
        left: ExpressionReplacement,
        right: ExpressionReplacement,
        left_ty: Option<SymbolType>,
        right_ty: Option<SymbolType>,
    ) -> ExpressionReplacement {
        match self.mode {
            ModuloLoweringMode::BuiltinFmod => ExpressionReplacement::changed_text("fmod(")
                .with_replacement(left)
                .with_text(", ")
                .with_replacement(right)
                .with_text(")"),
            ModuloLoweringMode::NagaCompatible => {
                let left = left.into_float_operand(left_ty);
                let right = right.into_float_operand(right_ty);
                ExpressionReplacement::changed_text("((")
                    .with_replacement(left.clone())
                    .with_text(") - (")
                    .with_replacement(right.clone())
                    .with_text(") * trunc((")
                    .with_replacement(left)
                    .with_text(") / (")
                    .with_replacement(right)
                    .with_text(")))")
            }
        }
    }
}
