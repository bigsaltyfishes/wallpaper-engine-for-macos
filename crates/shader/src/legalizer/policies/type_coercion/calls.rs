use super::{
    BalancedTokens, Fixup, FunctionCall, PolicyContext, SourceSpan, Token, TokenKind, TokenSearch,
    assignment::{AssignmentEquals, AssignmentLhs, LhsExpression, LhsWidth},
    initializer::FloatLiteral,
    types::{
        BindingType, ExpressionWidthFacts, SourcePointExt, SwizzledTokenRange, TokenRange,
        TokenRangeFacts, VectorTypeBindings, VectorWidth,
    },
};

#[derive(Clone, Copy)]
/// Builtin function whose scalar arguments are coerced in legacy shaders.
pub(super) struct FunctionCoercion<'module, 'src> {
    /// Original function call.
    pub(super) call: FunctionCall<'module, 'src>,
    /// Coercion rules for this function.
    pub(super) function: CoercionFunction,
    /// Known vector declarations.
    pub(super) vector_facts: &'module VectorTypeBindings<'src>,
}
impl FunctionCoercion<'_, '_> {
    /// Emits numeric literal and scalar broadcast fixups for this call.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        let arguments = CallArguments::from(self.call);
        let width = self
            .function
            .broadcasts()
            .then(|| arguments.coercion_width(self.call, self.vector_facts, self.function))
            .flatten();

        for (index, argument) in arguments.iter().enumerate() {
            if let Ok(literal) = NumericLiteral::try_from(ArgumentTokens {
                argument,
                tokens: self.call.tokens,
            }) && (self.function.promotes_integer_literals() || width.is_some())
            {
                literal.emit_float(context, self.function.broadcast_width(width, index));
            }
        }

        let Some(width) = width else {
            return;
        };

        for (index, argument) in arguments.iter().enumerate() {
            let Some(width) = self.function.broadcast_width(Some(width), index) else {
                continue;
            };
            let expression = ArgumentExpression::new(argument, self.call.tokens, self.vector_facts);
            if expression.vector_width().is_some() || !expression.is_scalar_like() {
                continue;
            }
            if NumericLiteral::try_from(ArgumentTokens {
                argument,
                tokens: self.call.tokens,
            })
            .is_ok()
            {
                continue;
            }
            expression.emit_vector_constructor(context, width);
        }

        if !self.function.narrows_vector_arguments() {
            return;
        }
        for argument in arguments.iter() {
            let expression = ArgumentExpression::new(argument, self.call.tokens, self.vector_facts);
            if expression
                .vector_width()
                .is_none_or(|argument_width| argument_width <= width)
            {
                continue;
            }
            expression.emit_narrow_swizzle(context, width);
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Type coercion behavior for one builtin name.
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
#[derive(Clone, Debug, Eq, PartialEq)]
/// Top-level argument list for one function call.
pub(super) struct CallArguments {
    /// Arguments in source order.
    pub(super) items: Vec<CallArgument>,
}
impl CallArguments {
    /// Iterates top-level arguments.
    pub(super) fn iter(&self) -> impl Iterator<Item = CallArgument> + '_ {
        self.items.iter().copied()
    }

    /// Returns the vector width that builtin arguments should conform to.
    pub(super) fn coercion_width(
        &self,
        call: FunctionCall<'_, '_>,
        facts: &VectorTypeBindings<'_>,
        function: CoercionFunction,
    ) -> Option<VectorWidth> {
        let context_width = CallContextWidth::try_from(CallContext { call, facts })
            .ok()
            .map(|width| width.width);
        if let Some(context_width) = context_width {
            return self
                .items
                .iter()
                .copied()
                .filter_map(|argument| {
                    ArgumentExpression::new(argument, call.tokens, facts).vector_width()
                })
                .all(|argument_width| argument_width >= context_width)
                .then_some(context_width);
        }
        let primary_argument_width = self
            .items
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, _argument)| function.selects_vector_width(*index))
            .filter_map(|(_index, argument)| {
                ArgumentExpression::new(argument, call.tokens, facts).vector_width()
            })
            .min();
        let special_argument_width = self
            .items
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, _argument)| !function.selects_vector_width(*index))
            .filter_map(|(_index, argument)| {
                ArgumentExpression::new(argument, call.tokens, facts).vector_width()
            })
            .min();
        match (primary_argument_width, special_argument_width) {
            (Some(primary), Some(special)) => Some(primary.min(special)),
            (Some(width), None) | (None, Some(width)) => Some(width),
            (None, None) => None,
        }
    }
}
#[derive(Clone, Copy)]
/// Context surrounding a builtin call expression.
pub(super) struct CallContext<'module, 'facts, 'src> {
    /// Function call whose result may have assignment context.
    pub(super) call: FunctionCall<'module, 'src>,
    /// Known vector declarations.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Vector width required by the expression receiving a call result.
pub(super) struct CallContextWidth {
    /// Expected vector width.
    pub(super) width: VectorWidth,
}
impl TryFrom<CallContext<'_, '_, '_>> for CallContextWidth {
    type Error = ();

    fn try_from(context: CallContext<'_, '_, '_>) -> Result<Self, Self::Error> {
        let tokens = context.call.tokens;
        let search = TokenSearch::new(tokens);
        if search
            .next_non_comment(context.call.close_index + 1)
            .is_some_and(|next| matches!(tokens[next].kind, TokenKind::Punctuation('.' | '[')))
        {
            return Err(());
        }
        let equals = search
            .previous_non_comment(context.call.name_index)
            .ok_or(())?;
        if !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
            return Err(());
        }
        let lhs_end = AssignmentLhs::try_from(AssignmentEquals { tokens, equals })?.end;
        LhsWidth::try_from(LhsExpression {
            tokens,
            end: lhs_end,
            facts: context.facts,
        })
        .map(|width| Self { width: width.width })
    }
}
impl From<FunctionCall<'_, '_>> for CallArguments {
    fn from(call: FunctionCall<'_, '_>) -> Self {
        let mut items = Vec::new();
        let mut start = call.open_index + 1;
        let mut depth = 0usize;

        for index in call.open_index + 1..call.close_index {
            match call.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => {
                    if let Some(argument) = CallArgument::from_bounds(call.tokens, start, index) {
                        items.push(argument);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        if let Some(argument) = CallArgument::from_bounds(call.tokens, start, call.close_index) {
            items.push(argument);
        }

        Self { items }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One top-level function argument.
pub(super) struct CallArgument {
    /// First token index.
    pub(super) start: usize,
    /// Last token index.
    pub(super) end: usize,
    /// Source span covering the argument.
    pub(super) span: SourceSpan,
}
impl CallArgument {
    /// Creates an argument from possibly-comment-padded token bounds.
    pub(super) fn from_bounds(tokens: &[Token<'_>], start: usize, end: usize) -> Option<Self> {
        let search = TokenSearch::new(tokens);
        let start = search.next_non_comment(start)?;
        let end = search.previous_non_comment(end)?;
        if start > end {
            return None;
        }
        let span = SourceSpan::new(tokens[start].span.start(), tokens[end].span.end()).ok()?;
        Some(Self { start, end, span })
    }

    /// Returns whether the argument is exactly one non-comment token.
    pub(super) const fn is_single_token(self) -> bool {
        self.start == self.end
    }
}
impl TokenRangeFacts for CallArgument {
    fn start(self) -> usize {
        self.start
    }

    fn end(self) -> usize {
        self.end
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Parsed numeric literal argument.
pub(super) struct NumericLiteral<'src> {
    /// Literal source text.
    pub(super) text: &'src str,
    /// Source span of the literal.
    pub(super) span: SourceSpan,
}
impl NumericLiteral<'_> {
    /// Emits a float-literal replacement, optionally wrapped in a vector
    /// constructor.
    pub(super) fn emit_float(
        self,
        context: &mut PolicyContext<'_, '_, '_>,
        width: Option<VectorWidth>,
    ) {
        let value = FloatLiteral { text: self.text }.to_string();
        let replacement = width.map_or(value.clone(), |width| {
            format!("{}({value})", width.constructor())
        });
        context
            .context()
            .fixups
            .push(Fixup::replace(self.span, replacement));
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token view for one call argument.
pub(super) struct ArgumentTokens<'module, 'src> {
    /// Argument being inspected.
    pub(super) argument: CallArgument,
    /// Token stream containing the argument.
    pub(super) tokens: &'module [Token<'src>],
}
impl<'src> TryFrom<ArgumentTokens<'_, 'src>> for NumericLiteral<'src> {
    type Error = ();

    fn try_from(value: ArgumentTokens<'_, 'src>) -> Result<Self, Self::Error> {
        if !value.argument.is_single_token() {
            return Err(());
        }
        let TokenKind::Number(text) = value.tokens[value.argument.start].kind else {
            return Err(());
        };
        if text
            .bytes()
            .any(|byte| matches!(byte, b'.' | b'e' | b'E' | b'f' | b'F'))
        {
            return Err(());
        }
        Ok(Self {
            text,
            span: value.tokens[value.argument.start].span,
        })
    }
}
#[derive(Clone, Copy)]
/// Lightweight expression classifier for one top-level argument.
pub(super) struct ArgumentExpression<'module, 'src> {
    /// Argument being classified.
    pub(super) argument: CallArgument,
    /// Token stream containing the argument.
    pub(super) tokens: &'module [Token<'src>],
    /// Known vector declarations.
    pub(super) facts: &'module VectorTypeBindings<'src>,
}
impl<'module, 'src> ArgumentExpression<'module, 'src> {
    /// Creates an expression classifier.
    pub(super) const fn new(
        argument: CallArgument,
        tokens: &'module [Token<'src>],
        facts: &'module VectorTypeBindings<'src>,
    ) -> Self {
        Self {
            argument,
            tokens,
            facts,
        }
    }

    /// Returns vector width when the argument is an explicit vector constructor
    /// call.
    pub(super) fn vector_width(self) -> Option<VectorWidth> {
        if self.argument.start() == self.argument.end() {
            let TokenKind::Identifier(name) = self.tokens[self.argument.start()].kind else {
                return None;
            };
            return match self.facts.lookup(name, self.argument.start()) {
                Some(BindingType::Vector(width)) => Some(width),
                Some(BindingType::Scalar | BindingType::Blocker) | None => None,
            };
        }
        if let Ok(swizzled) = SwizzledTokenRange::try_from(TokenRange {
            tokens: self.tokens,
            start: self.argument.start(),
            end: self.argument.end(),
        }) && self.has_swizzled_vector_base(swizzled)
        {
            return Some(swizzled.width);
        }
        let TokenKind::Identifier(name) = self.tokens[self.argument.start()].kind else {
            return None;
        };
        let width = VectorWidth::from_constructor(name)?;
        matches!(
            (
                self.tokens[self.argument.start() + 1].kind,
                self.tokens[self.argument.end()].kind
            ),
            (TokenKind::LeftParen, TokenKind::RightParen)
        )
        .then_some(width)
    }

    /// Returns whether the argument is safe to treat as a scalar expression.
    pub(super) fn is_scalar_like(self) -> bool {
        if self.argument.is_single_token() {
            return self.single_token_scalar_like(self.argument.start);
        }

        self.is_parenthesized_scalar() || self.is_scalar_expression()
    }

    /// Emits constructor insertions broadcasting this argument.
    pub(super) fn emit_vector_constructor(
        self,
        context: &mut PolicyContext<'_, '_, '_>,
        width: VectorWidth,
    ) {
        context.context().fixups.push(Fixup::insert(
            self.argument.span.start_point(),
            format!("{}(", width.constructor()),
        ));
        context.context().fixups.push(Fixup::insert(
            self.argument.span.end_point(),
            ")".to_owned(),
        ));
    }

    /// Emits a trailing swizzle that narrows this vector expression.
    pub(super) fn emit_narrow_swizzle(
        self,
        context: &mut PolicyContext<'_, '_, '_>,
        width: VectorWidth,
    ) {
        let Some(swizzle) = width.narrow_swizzle() else {
            return;
        };
        let suffix = if self.needs_parentheses_for_swizzle() {
            context.context().fixups.push(Fixup::insert(
                self.argument.span.start_point(),
                "(".to_owned(),
            ));
            format!("){swizzle}")
        } else {
            swizzle.to_owned()
        };
        context
            .context()
            .fixups
            .push(Fixup::insert(self.argument.span.end_point(), suffix));
    }

    /// Returns whether the expression is a simple parenthesized scalar-like
    /// value.
    pub(super) fn is_parenthesized_scalar(self) -> bool {
        if !matches!(self.tokens[self.argument.start].kind, TokenKind::LeftParen)
            || !matches!(self.tokens[self.argument.end].kind, TokenKind::RightParen)
        {
            return false;
        }
        let Some(inner) =
            CallArgument::from_bounds(self.tokens, self.argument.start + 1, self.argument.end)
        else {
            return false;
        };
        Self::new(inner, self.tokens, self.facts).is_scalar_like()
    }

    /// Returns whether an expression's top-level operators are scalar-only and
    /// every identifier operand is known scalar or unresolved.
    pub(super) fn is_scalar_expression(self) -> bool {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut saw_scalar_operator = false;
        let mut index = self.argument.start;

        while index <= self.argument.end {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TokenKind::Identifier(name) => {
                    if let Some(close) = self.scalar_call_end(index) {
                        index = close + 1;
                        continue;
                    }
                    if !self.identifier_scalar_like(index, name, paren_depth, bracket_depth) {
                        return false;
                    }
                }
                TokenKind::Punctuation('.' | '?' | ':') => return false,
                TokenKind::Punctuation('<' | '>' | '=' | '&' | '|' | '^' | '!')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return false;
                }
                TokenKind::Punctuation('+' | '-' | '*' | '/' | '%')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    saw_scalar_operator = true;
                }
                _ => {}
            }
            index += 1;
        }

        saw_scalar_operator
    }

    /// Returns whether a one-token expression is known scalar-like.
    pub(super) fn single_token_scalar_like(self, index: usize) -> bool {
        let TokenKind::Identifier(name) = self.tokens[index].kind else {
            return matches!(self.tokens[index].kind, TokenKind::Number(_));
        };
        self.identifier_scalar_like(index, name, 0, 0)
    }

    /// Returns whether an identifier reference is known scalar-like in this
    /// argument. Nested calls are treated as scalar-compatible because legacy
    /// shaders commonly use scalar helpers inside vector builtins.
    pub(super) fn identifier_scalar_like(
        self,
        index: usize,
        name: &str,
        paren_depth: usize,
        bracket_depth: usize,
    ) -> bool {
        if self.is_function_name(index) && (paren_depth > 0 || bracket_depth == 0) {
            return true;
        }
        matches!(
            self.facts.lookup(name, index),
            Some(BindingType::Scalar) | None
        )
    }

    /// Returns whether the identifier at `index` is followed by a call
    /// argument list.
    pub(super) fn is_function_name(self, index: usize) -> bool {
        TokenSearch::new(self.tokens)
            .next_non_comment(index + 1)
            .is_some_and(|next| matches!(self.tokens[next].kind, TokenKind::LeftParen))
    }

    /// Returns the closing parenthesis for a nested call that can be treated as
    /// a scalar expression leaf.
    pub(super) fn scalar_call_end(self, index: usize) -> Option<usize> {
        let TokenKind::Identifier(name) = self.tokens[index].kind else {
            return None;
        };
        if VectorWidth::from_constructor(name).is_some() {
            return None;
        }
        let open = TokenSearch::new(self.tokens).next_non_comment(index + 1)?;
        if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
            return None;
        }
        BalancedTokens::new(self.tokens).matching_right_paren(open)
    }

    /// Returns whether a swizzle must be applied to a parenthesized expression.
    pub(super) fn needs_parentheses_for_swizzle(self) -> bool {
        !self.argument.is_single_token()
    }

    /// Returns whether a swizzled argument starts from a visible vector base.
    pub(super) fn has_swizzled_vector_base(self, swizzled: SwizzledTokenRange) -> bool {
        let TokenKind::Identifier(base) = self.tokens[swizzled.base].kind else {
            return false;
        };
        matches!(
            self.facts.lookup(base, swizzled.base),
            Some(BindingType::Vector(_))
        )
    }
}
impl ExpressionWidthFacts for ArgumentExpression<'_, '_> {
    fn vector_width(self) -> Option<VectorWidth> {
        self.vector_width()
    }
}
