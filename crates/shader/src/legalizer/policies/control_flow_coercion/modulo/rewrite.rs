use super::{
    BalancedTokens, ExpressionReplacement, SourceSpan, SymbolFacts, SymbolType, Token, TokenKind,
    TokenSearch,
    operands::{ModuloExpression, StatementInitializer},
    typing::ExpressionType,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Replacement fragment emitted while lowering one modulo expression.
pub(super) struct ExpressionSource {
    /// Ordered expression fragments.
    pub(super) parts: Vec<ExpressionSourcePart>,
    /// Whether generated text changed the expression semantics.
    pub(super) changed: bool,
}
impl ExpressionSource {
    /// Creates an expression fragment from literal text.
    pub(super) fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![ExpressionSourcePart::Text(text.into())],
            changed: true,
        }
    }

    /// Appends literal text.
    pub(super) fn push_text(&mut self, text: impl Into<String>) {
        self.parts.push(ExpressionSourcePart::Text(text.into()));
    }

    /// Appends an original source span.
    pub(super) fn push_source(&mut self, span: SourceSpan) {
        self.parts.push(ExpressionSourcePart::Source(span));
    }

    /// Appends another expression fragment.
    pub(super) fn push_expression(&mut self, expression: Self) {
        self.changed |= expression.changed;
        self.parts.extend(expression.parts);
    }

    /// Appends literal text and returns the updated expression.
    pub(super) fn with_text(mut self, text: impl Into<String>) -> Self {
        self.push_text(text);
        self
    }

    /// Appends another expression fragment and returns the updated expression.
    pub(super) fn with_expression(mut self, expression: Self) -> Self {
        self.push_expression(expression);
        self
    }

    /// Converts this fragment to the shared expression replacement type.
    pub(super) fn replacement(self) -> ExpressionReplacement {
        let mut replacement = ExpressionReplacement::new();
        for part in self.parts {
            replacement = match part {
                ExpressionSourcePart::Text(text) => replacement.with_text(text),
                ExpressionSourcePart::Source(span) => replacement.with_source(span),
            };
        }
        replacement
    }

    /// Returns whether this expression contains generated semantic changes.
    pub(super) fn is_changed(&self) -> bool {
        self.changed
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// One replacement fragment emitted while lowering modulo.
pub(super) enum ExpressionSourcePart {
    /// Literal replacement text.
    Text(String),
    /// Original source span rendered with child fixups.
    Source(SourceSpan),
}
/// Initializer being lowered from `%` to `fmod`.
pub(super) struct ModuloInitializer<'expression, 'tokens, 'src> {
    /// Source text being rewritten.
    pub(super) expression: &'expression ModuloExpression,
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Initializer bounds.
    pub(super) initializer: StatementInitializer,
    /// Known symbol facts.
    pub(super) facts: &'expression SymbolFacts<'src>,
    /// Modulo lowering style.
    pub(super) mode: ModuloLoweringMode,
}
impl ModuloInitializer<'_, '_, '_> {
    /// Lowers all `%` operators inside this initializer.
    pub(super) fn lower(self) -> Result<ExpressionSource, ()> {
        ModuloRange {
            expression: self.expression,
            tokens: self.tokens,
            start: self.initializer.start,
            end: self.initializer.end,
            facts: self.facts,
            mode: self.mode,
        }
        .lower()
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
/// Token range whose nested modulo expressions can be lowered.
#[derive(Clone, Copy)]
pub(super) struct ModuloRange<'expression, 'tokens, 'src> {
    /// Source text being rewritten.
    pub(super) expression: &'expression ModuloExpression,
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token in the range.
    pub(super) start: usize,
    /// Last token in the range.
    pub(super) end: usize,
    /// Known symbol facts.
    pub(super) facts: &'expression SymbolFacts<'src>,
    /// Modulo lowering style.
    pub(super) mode: ModuloLoweringMode,
}
impl ModuloRange<'_, '_, '_> {
    /// Lowers `%` operators while preserving source outside affected segments.
    pub(super) fn lower(self) -> Result<ExpressionSource, ()> {
        let mut output = ExpressionSource::default();
        let range_span = SourceSpan::new(
            self.tokens[self.start].span.start(),
            self.tokens[self.end].span.end(),
        )
        .map_err(|_error| ())?;
        let mut copied = range_span.start();
        let mut segment_start = self.start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        for (index, token) in self
            .tokens
            .iter()
            .enumerate()
            .take(self.end + 1)
            .skip(self.start)
        {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1).ok_or(())?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.checked_sub(1).ok_or(())?;
                }
                TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::Punctuation('=' | '?' | ':' | '<' | '>')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    ModuloSegment {
                        expression: self.expression,
                        tokens: self.tokens,
                        start: segment_start,
                        end: index.saturating_sub(1),
                        facts: self.facts,
                        mode: self.mode,
                    }
                    .append(&mut copied, &mut output)?;
                    segment_start = index + 1;
                }
                TokenKind::Punctuation('+' | '-')
                    if paren_depth == 0
                        && bracket_depth == 0
                        && !self.is_unary_sign(index, segment_start) =>
                {
                    ModuloSegment {
                        expression: self.expression,
                        tokens: self.tokens,
                        start: segment_start,
                        end: index.saturating_sub(1),
                        facts: self.facts,
                        mode: self.mode,
                    }
                    .append(&mut copied, &mut output)?;
                    segment_start = index + 1;
                }
                _ => {}
            }
        }

        ModuloSegment {
            expression: self.expression,
            tokens: self.tokens,
            start: segment_start,
            end: self.end,
            facts: self.facts,
            mode: self.mode,
        }
        .append(&mut copied, &mut output)?;
        output.push_source(SourceSpan::new(copied, range_span.end()).map_err(|_error| ())?);
        Ok(output)
    }

    /// Lowers `%` operators inside nested balanced delimiters only.
    pub(super) fn lower_nested(self) -> Result<ExpressionSource, ()> {
        let range_span = SourceSpan::new(
            self.tokens[self.start].span.start(),
            self.tokens[self.end].span.end(),
        )
        .map_err(|_error| ())?;
        let mut output = ExpressionSource::default();
        let mut copied = range_span.start();
        let mut index = self.start;
        while index <= self.end {
            if self.is_texture_call_open(index) {
                index += 1;
                continue;
            }
            let Some(close) = self.balanced_close(index) else {
                index += 1;
                continue;
            };
            output.push_source(
                SourceSpan::new(copied, self.tokens[index].span.end()).map_err(|_error| ())?,
            );
            let inner_start = TokenSearch::new(self.tokens).next_non_comment(index + 1);
            let inner_end = TokenSearch::new(self.tokens).previous_non_comment(close);
            if let (Some(inner_start), Some(inner_end)) = (inner_start, inner_end)
                && inner_start <= inner_end
            {
                output.push_expression(
                    ModuloRange {
                        expression: self.expression,
                        tokens: self.tokens,
                        start: inner_start,
                        end: inner_end,
                        facts: self.facts,
                        mode: self.mode,
                    }
                    .lower()?,
                );
            }
            copied = self.tokens[close].span.start();
            index = close + 1;
        }
        output.push_source(SourceSpan::new(copied, range_span.end()).map_err(|_error| ())?);
        Ok(output)
    }

    /// Returns whether `index` is the argument-list opener for a texture
    /// sampling call. Those parentheses belong to the same source fragment so
    /// texture sampler insertions at argument boundaries are rendered once.
    pub(super) fn is_texture_call_open(self, index: usize) -> bool {
        if !matches!(self.tokens[index].kind, TokenKind::LeftParen) {
            return false;
        }
        let Some(previous) = TokenSearch::new(self.tokens).previous_non_comment(index) else {
            return false;
        };
        matches!(
            self.tokens[previous].kind,
            TokenKind::Identifier(
                "texture"
                    | "texture2D"
                    | "tex2D"
                    | "texSample2D"
                    | "textureLod"
                    | "texture2DLod"
                    | "tex2DLod"
                    | "texSample2DLod"
            )
        )
    }

    /// Returns the matching close token for a balanced delimiter at `index`.
    pub(super) fn balanced_close(self, index: usize) -> Option<usize> {
        let balanced = BalancedTokens::new(self.tokens);
        let close = match self.tokens[index].kind {
            TokenKind::LeftParen => balanced.matching_right_paren(index),
            _ => None,
        }?;
        (close <= self.end).then_some(close)
    }

    /// Returns whether `+` or `-` starts a unary expression in the current
    /// segment.
    pub(super) fn is_unary_sign(self, index: usize, segment_start: usize) -> bool {
        if !matches!(self.tokens[index].kind, TokenKind::Punctuation('+' | '-')) {
            return false;
        }
        let Some(previous) = TokenSearch::new(self.tokens).previous_non_comment(index) else {
            return true;
        };
        if previous < segment_start {
            return true;
        }
        matches!(
            self.tokens[previous].kind,
            TokenKind::LeftParen
                | TokenKind::Comma
                | TokenKind::Punctuation('=' | '?' | ':' | '<' | '>' | '+' | '-' | '*' | '/' | '%')
        )
    }
}
/// One top-level multiplicative segment.
#[derive(Clone, Copy)]
pub(super) struct ModuloSegment<'expression, 'tokens, 'src> {
    /// Source text being rewritten.
    pub(super) expression: &'expression ModuloExpression,
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token in the segment.
    pub(super) start: usize,
    /// Last token in the segment.
    pub(super) end: usize,
    /// Known symbol facts.
    pub(super) facts: &'expression SymbolFacts<'src>,
    /// Modulo lowering style.
    pub(super) mode: ModuloLoweringMode,
}
impl ModuloSegment<'_, '_, '_> {
    /// Appends this segment, lowering `%` left-associatively.
    pub(super) fn append(
        self,
        copied: &mut usize,
        output: &mut ExpressionSource,
    ) -> Result<(), ()> {
        let Some(start) = TokenSearch::new(self.tokens).next_non_comment(self.start) else {
            return Ok(());
        };
        if start > self.end {
            return Ok(());
        }
        let end = TokenSearch::new(self.tokens)
            .previous_non_comment(self.end + 1)
            .ok_or(())?;
        let segment_span =
            SourceSpan::new(self.tokens[start].span.start(), self.tokens[end].span.end())
                .map_err(|_error| ())?;
        output.push_source(SourceSpan::new(*copied, segment_span.start()).map_err(|_error| ())?);
        output.push_expression(self.lower(start, end)?);
        *copied = segment_span.end();
        Ok(())
    }

    /// Lowers `%` operators inside this multiplicative segment.
    pub(super) fn lower(self, start: usize, end: usize) -> Result<ExpressionSource, ()> {
        let operators = self.operators(start, end)?;
        if !operators
            .iter()
            .any(|operator| matches!(self.tokens[*operator].kind, TokenKind::Punctuation('%')))
        {
            return ModuloRange {
                expression: self.expression,
                tokens: self.tokens,
                start,
                end,
                facts: self.facts,
                mode: self.mode,
            }
            .lower_nested();
        }

        let first_operator = *operators.first().ok_or(())?;
        let mut acc = self.operand_expression(start, first_operator)?;
        let mut acc_ty = self.expression_type_before(start, first_operator);
        for (position, operator) in operators.iter().enumerate() {
            let next_operator = operators.get(position + 1).copied();
            let operand_end = next_operator.unwrap_or(end + 1);
            let right = self.operand_expression(operator + 1, operand_end)?;
            let right_ty = self.expression_type_before(operator + 1, operand_end);
            match self.tokens[*operator].kind {
                TokenKind::Punctuation('%')
                    if SymbolType::integer_modulo_operands(acc_ty, right_ty) =>
                {
                    acc = ExpressionSource::binary(acc, " % ", right);
                    acc_ty = Some(SymbolType::integer_result(acc_ty, right_ty));
                }
                TokenKind::Punctuation('%') => {
                    acc = self.float_modulo(acc, right, acc_ty, right_ty);
                    acc_ty = Some(SymbolType::Float);
                }
                TokenKind::Punctuation(operator) => {
                    acc = ExpressionSource::binary(acc, format!(" {operator} "), right);
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
    pub(super) fn expression_type_before(self, start: usize, end: usize) -> Option<SymbolType> {
        let start = TokenSearch::new(self.tokens).next_non_comment(start)?;
        let end = TokenSearch::new(self.tokens).previous_non_comment(end)?;
        if start > end {
            return None;
        }
        ExpressionType {
            tokens: self.tokens,
            facts: self.facts,
        }
        .range_type(start, end)
    }

    /// Returns all top-level multiplicative operators in this segment.
    pub(super) fn operators(self, start: usize, end: usize) -> Result<Vec<usize>, ()> {
        let mut operators = Vec::new();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().take(end + 1).skip(start) {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1).ok_or(())?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.checked_sub(1).ok_or(())?;
                }
                TokenKind::Punctuation('*' | '/' | '%')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    operators.push(index);
                }
                _ => {}
            }
        }
        Ok(operators)
    }

    /// Returns trimmed and recursively lowered source for one multiplicative
    /// operand.
    pub(super) fn operand_expression(
        self,
        start: usize,
        end: usize,
    ) -> Result<ExpressionSource, ()> {
        let start = TokenSearch::new(self.tokens)
            .next_non_comment(start)
            .ok_or(())?;
        let end = TokenSearch::new(self.tokens)
            .previous_non_comment(end)
            .ok_or(())?;
        ModuloRange {
            expression: self.expression,
            tokens: self.tokens,
            start,
            end,
            facts: self.facts,
            mode: self.mode,
        }
        .lower_nested()
    }

    /// Emits one non-integer modulo expression.
    pub(super) fn float_modulo(
        self,
        left: ExpressionSource,
        right: ExpressionSource,
        left_ty: Option<SymbolType>,
        right_ty: Option<SymbolType>,
    ) -> ExpressionSource {
        match self.mode {
            ModuloLoweringMode::BuiltinFmod => ExpressionSource::text("fmod(")
                .with_expression(left)
                .with_text(", ")
                .with_expression(right)
                .with_text(")"),
            ModuloLoweringMode::NagaCompatible => {
                let left = left.into_float_operand(left_ty);
                let right = right.into_float_operand(right_ty);
                ExpressionSource::text("((")
                    .with_expression(left.clone())
                    .with_text(") - (")
                    .with_expression(right.clone())
                    .with_text(") * trunc((")
                    .with_expression(left)
                    .with_text(") / (")
                    .with_expression(right)
                    .with_text(")))")
            }
        }
    }
}
