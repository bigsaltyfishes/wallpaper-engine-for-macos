//! Conditional directive stack and expression parsing.

use super::{DirectiveLocation, MacroTable};

/// Active conditional state for line-oriented preprocessing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConditionalStack<'a> {
    /// Nested conditional frames from outermost to innermost.
    frames: Vec<ConditionalFrame<'a>>,
}

impl<'a> ConditionalStack<'a> {
    /// Creates an empty conditional stack.
    #[must_use]
    pub const fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Returns whether the current source line is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.frames
            .last()
            .is_none_or(|frame| frame.parent_active && frame.condition_active)
    }

    /// Pushes a new conditional frame.
    pub(super) fn push(&mut self, condition_active: bool, opening: DirectiveLocation<'a>) {
        let parent_active = self.is_active();
        self.frames.push(ConditionalFrame {
            parent_active,
            condition_active,
            branch_state: if condition_active {
                ConditionalBranchState::BranchSelected
            } else {
                ConditionalBranchState::NoBranchSelected
            },
            opening,
        });
    }

    /// Enters an `#elif` arm for the current frame.
    pub(super) fn enter_elif(&mut self, condition_active: bool) -> Result<(), ConditionalError> {
        let Some(frame) = self.frames.last_mut() else {
            return Err(ConditionalError::UnmatchedElif);
        };

        if frame.branch_state == ConditionalBranchState::ElseSeen {
            return Err(ConditionalError::ElifAfterElse);
        }

        frame.condition_active =
            frame.parent_active && !frame.branch_state.is_selected() && condition_active;
        if frame.condition_active {
            frame.branch_state = ConditionalBranchState::BranchSelected;
        }
        Ok(())
    }

    /// Returns whether the next `#elif` expression can affect output.
    pub(super) fn should_evaluate_elif(&self) -> Result<bool, ConditionalError> {
        let Some(frame) = self.frames.last() else {
            return Err(ConditionalError::UnmatchedElif);
        };

        if frame.branch_state == ConditionalBranchState::ElseSeen {
            return Err(ConditionalError::ElifAfterElse);
        }

        Ok(frame.parent_active && !frame.branch_state.is_selected())
    }

    /// Enters the `#else` arm for the current frame.
    pub(super) fn enter_else(&mut self) -> Result<(), ConditionalError> {
        let Some(frame) = self.frames.last_mut() else {
            return Err(ConditionalError::UnmatchedElse);
        };

        if frame.branch_state == ConditionalBranchState::ElseSeen {
            return Err(ConditionalError::DuplicateElse);
        }

        frame.condition_active = frame.parent_active && !frame.branch_state.is_selected();
        frame.branch_state = ConditionalBranchState::ElseSeen;
        Ok(())
    }

    /// Pops the current conditional frame.
    pub(super) fn pop(&mut self) -> Result<(), ConditionalError> {
        if self.frames.pop().is_none() {
            return Err(ConditionalError::UnmatchedEndif);
        }
        Ok(())
    }

    /// Returns whether no conditional frames are active.
    pub(super) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns the innermost unclosed directive location.
    pub(super) fn innermost_opening(&self) -> Option<DirectiveLocation<'a>> {
        self.frames.last().map(|frame| frame.opening)
    }
}

/// One active conditional directive frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ConditionalFrame<'a> {
    /// Whether all enclosing parent conditionals are active.
    parent_active: bool,
    /// Whether this frame's own condition is active.
    condition_active: bool,
    /// Whether this frame has selected a prior branch or consumed `#else`.
    branch_state: ConditionalBranchState,
    /// Location of the opening directive for diagnostics.
    opening: DirectiveLocation<'a>,
}

/// Branch selection state for one conditional directive frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConditionalBranchState {
    /// No branch in the frame has been selected yet.
    NoBranchSelected,
    /// A previous `#if` or `#elif` branch was selected.
    BranchSelected,
    /// The frame has consumed its final `#else` arm.
    ElseSeen,
}

impl ConditionalBranchState {
    /// Returns whether any branch in this frame has already been selected.
    const fn is_selected(self) -> bool {
        matches!(self, Self::BranchSelected | Self::ElseSeen)
    }
}

/// Errors produced while balancing conditional directives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConditionalError {
    /// `#elif` appeared without a matching opening directive.
    UnmatchedElif,
    /// `#elif` appeared after an `#else` arm.
    ElifAfterElse,
    /// `#else` appeared without a matching opening directive.
    UnmatchedElse,
    /// A conditional frame contained more than one `#else`.
    DuplicateElse,
    /// `#endif` appeared without a matching opening directive.
    UnmatchedEndif,
}

/// How conditional directive blocks affect emitted source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConditionalMode {
    /// Evaluate conditionals and emit only active branches.
    Evaluate,
    /// Preserve conditional directive lines and all branch bodies.
    Preserve,
}

/// Supported `#if` conditional expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConditionalExpression<'src> {
    /// Raw expression text.
    source: &'src str,
}

impl ConditionalExpression<'_> {
    /// Evaluates the expression against visible macros.
    pub(super) fn evaluate(self, macros: &MacroTable) -> Result<bool, &'static str> {
        ConditionalExpressionParser::try_from(ConditionalExpressionContext {
            source: self.source,
            macros,
        })?
        .parse()
    }
}

impl<'src> TryFrom<&'src str> for ConditionalExpression<'src> {
    type Error = &'static str;

    fn try_from(source: &'src str) -> Result<Self, Self::Error> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err("#if expects an expression");
        }

        Ok(Self { source: trimmed })
    }
}

/// Token in a supported `#if` expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConditionalToken<'src> {
    /// Macro identifier.
    Identifier(&'src str),
    /// Signed integer value parsed from a decimal literal.
    Integer(i64),
    /// `||`.
    Or,
    /// `&&`.
    And,
    /// `!`.
    Not,
    /// `==`.
    Equal,
    /// `!=`.
    NotEqual,
    /// `<`.
    Less,
    /// `<=`.
    LessEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterEqual,
    /// `(`.
    OpenParen,
    /// `)`.
    CloseParen,
}

/// Token stream for a supported `#if` expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ConditionalTokens<'src> {
    /// Parsed tokens.
    items: Vec<ConditionalToken<'src>>,
}

impl<'src> TryFrom<&'src str> for ConditionalTokens<'src> {
    type Error = &'static str;

    fn try_from(source: &'src str) -> Result<Self, Self::Error> {
        let mut tokens = Vec::new();
        let bytes = source.as_bytes();
        let mut position = 0;

        while position < bytes.len() {
            match bytes[position] {
                byte if byte.is_ascii_whitespace() => {
                    position += 1;
                }
                b'0'..=b'9' => {
                    let start = position;
                    position += 1;
                    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                        position += 1;
                    }
                    let value = source[start..position]
                        .parse::<i64>()
                        .map_err(|_error| "#if expression is unsupported")?;
                    tokens.push(ConditionalToken::Integer(value));
                }
                b'_' | b'A'..=b'Z' | b'a'..=b'z' => {
                    let start = position;
                    position += 1;
                    while bytes
                        .get(position)
                        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
                    {
                        position += 1;
                    }
                    tokens.push(ConditionalToken::Identifier(&source[start..position]));
                }
                b'|' if bytes.get(position + 1).is_some_and(|byte| *byte == b'|') => {
                    tokens.push(ConditionalToken::Or);
                    position += 2;
                }
                b'&' if bytes.get(position + 1).is_some_and(|byte| *byte == b'&') => {
                    tokens.push(ConditionalToken::And);
                    position += 2;
                }
                b'!' if bytes.get(position + 1).is_some_and(|byte| *byte == b'=') => {
                    tokens.push(ConditionalToken::NotEqual);
                    position += 2;
                }
                b'=' if bytes.get(position + 1).is_some_and(|byte| *byte == b'=') => {
                    tokens.push(ConditionalToken::Equal);
                    position += 2;
                }
                b'<' if bytes.get(position + 1).is_some_and(|byte| *byte == b'=') => {
                    tokens.push(ConditionalToken::LessEqual);
                    position += 2;
                }
                b'>' if bytes.get(position + 1).is_some_and(|byte| *byte == b'=') => {
                    tokens.push(ConditionalToken::GreaterEqual);
                    position += 2;
                }
                b'!' => {
                    tokens.push(ConditionalToken::Not);
                    position += 1;
                }
                b'<' => {
                    tokens.push(ConditionalToken::Less);
                    position += 1;
                }
                b'>' => {
                    tokens.push(ConditionalToken::Greater);
                    position += 1;
                }
                b'(' => {
                    tokens.push(ConditionalToken::OpenParen);
                    position += 1;
                }
                b')' => {
                    tokens.push(ConditionalToken::CloseParen);
                    position += 1;
                }
                _ => return Err("#if expression is unsupported"),
            }
        }
        Ok(Self { items: tokens })
    }
}

impl<'src> ConditionalTokens<'src> {
    /// Moves the token stream into the parser-owned vector.
    fn into_vec(self) -> Vec<ConditionalToken<'src>> {
        self.items
    }
}

/// Numeric or boolean value produced while evaluating a `#if` expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConditionalValue {
    /// Numeric value, when the expression can be used as a comparison operand.
    integer: Option<i64>,
    /// Boolean interpretation used by `&&`, `||`, `!`, and final `#if` result.
    truthy: bool,
}

impl ConditionalValue {
    /// Creates a value from an integer.
    const fn integer(value: i64) -> Self {
        Self {
            integer: Some(value),
            truthy: value != 0,
        }
    }

    /// Creates a value from a boolean result.
    const fn boolean(value: bool) -> Self {
        Self {
            integer: Some(if value { 1 } else { 0 }),
            truthy: value,
        }
    }

    /// Returns the boolean interpretation.
    const fn is_truthy(self) -> bool {
        self.truthy
    }

    /// Returns the numeric interpretation required by comparison operators.
    fn expect_integer(self) -> Result<i64, &'static str> {
        self.integer.ok_or("#if expression is unsupported")
    }
}

/// Comparison operator in a supported `#if` expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConditionalComparison {
    /// `==`.
    Equal,
    /// `!=`.
    NotEqual,
    /// `<`.
    Less,
    /// `<=`.
    LessEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterEqual,
}

impl ConditionalComparison {
    /// Applies this comparison to two integer operands.
    const fn evaluate(self, left: i64, right: i64) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Less => left < right,
            Self::LessEqual => left <= right,
            Self::Greater => left > right,
            Self::GreaterEqual => left >= right,
        }
    }
}

/// Borrowed context needed to create a conditional expression parser.
#[derive(Clone, Copy, Debug)]
pub(super) struct ConditionalExpressionContext<'src, 'macros> {
    /// Raw expression text.
    source: &'src str,
    /// Visible macro values.
    macros: &'macros MacroTable,
}

/// Recursive-descent parser/evaluator for the Wallpaper Engine `#if` dialect.
#[derive(Debug)]
pub(super) struct ConditionalExpressionParser<'src, 'macros> {
    /// Lexed expression tokens.
    tokens: Vec<ConditionalToken<'src>>,
    /// Next token index.
    position: usize,
    /// Visible preprocessor macro values.
    macros: &'macros MacroTable,
}

impl<'src, 'macros> TryFrom<ConditionalExpressionContext<'src, 'macros>>
    for ConditionalExpressionParser<'src, 'macros>
{
    type Error = &'static str;

    fn try_from(context: ConditionalExpressionContext<'src, 'macros>) -> Result<Self, Self::Error> {
        let tokens = ConditionalTokens::try_from(context.source)?.into_vec();
        Ok(Self {
            tokens,
            position: 0,
            macros: context.macros,
        })
    }
}

impl<'src> ConditionalExpressionParser<'src, '_> {
    /// Parses and evaluates one conditional expression.
    fn parse(&mut self) -> Result<bool, &'static str> {
        let value = self.parse_or()?;
        if self.peek().is_some() {
            return Err("#if expression is unsupported");
        }

        Ok(value.is_truthy())
    }

    /// Parses `||` expressions.
    fn parse_or(&mut self) -> Result<ConditionalValue, &'static str> {
        let mut value = self.parse_and()?;
        while self.consume_or() {
            let right = self.parse_and()?;
            value = ConditionalValue::boolean(value.is_truthy() || right.is_truthy());
        }
        Ok(value)
    }

    /// Parses `&&` expressions.
    fn parse_and(&mut self) -> Result<ConditionalValue, &'static str> {
        let mut value = self.parse_comparison()?;
        while self.consume_and() {
            let right = self.parse_comparison()?;
            value = ConditionalValue::boolean(value.is_truthy() && right.is_truthy());
        }
        Ok(value)
    }

    /// Parses comparison expressions.
    fn parse_comparison(&mut self) -> Result<ConditionalValue, &'static str> {
        let mut value = self.parse_unary()?;
        while let Some(comparison) = self.consume_comparison() {
            let right = self.parse_unary()?;
            value = ConditionalValue::boolean(
                comparison.evaluate(value.expect_integer()?, right.expect_integer()?),
            );
        }
        Ok(value)
    }

    /// Parses unary expressions.
    fn parse_unary(&mut self) -> Result<ConditionalValue, &'static str> {
        if self.consume_not() {
            let value = self.parse_unary()?;
            return Ok(ConditionalValue::boolean(!value.is_truthy()));
        }

        self.parse_primary()
    }

    /// Parses integer, identifier, and parenthesized expressions.
    fn parse_primary(&mut self) -> Result<ConditionalValue, &'static str> {
        match self.next() {
            Some(ConditionalToken::Integer(value)) => Ok(ConditionalValue::integer(value)),
            Some(ConditionalToken::Identifier(name)) => Ok(self.macro_value(name)),
            Some(ConditionalToken::OpenParen) => {
                let value = self.parse_or()?;
                match self.next() {
                    Some(ConditionalToken::CloseParen) => Ok(value),
                    _ => Err("#if expression is unsupported"),
                }
            }
            _ => Err("#if expression is unsupported"),
        }
    }

    /// Converts a macro identifier into an expression value.
    fn macro_value(&self, name: &str) -> ConditionalValue {
        let Some(value) = self.macros.value(name) else {
            return ConditionalValue::integer(0);
        };

        let trimmed = value.trim();
        trimmed.parse::<i64>().map_or_else(
            |_error| ConditionalValue {
                integer: None,
                truthy: !trimmed.is_empty() && trimmed != "0",
            },
            ConditionalValue::integer,
        )
    }

    /// Returns the next token without consuming it.
    fn peek(&self) -> Option<ConditionalToken<'src>> {
        self.tokens.get(self.position).copied()
    }

    /// Consumes and returns the next token.
    fn next(&mut self) -> Option<ConditionalToken<'src>> {
        let token = self.peek()?;
        self.position += 1;
        Some(token)
    }

    /// Consumes a `||` token if present.
    fn consume_or(&mut self) -> bool {
        if matches!(self.peek(), Some(ConditionalToken::Or)) {
            self.position += 1;
            return true;
        }
        false
    }

    /// Consumes a `&&` token if present.
    fn consume_and(&mut self) -> bool {
        if matches!(self.peek(), Some(ConditionalToken::And)) {
            self.position += 1;
            return true;
        }
        false
    }

    /// Consumes a `!` token if present.
    fn consume_not(&mut self) -> bool {
        if matches!(self.peek(), Some(ConditionalToken::Not)) {
            self.position += 1;
            return true;
        }
        false
    }

    /// Consumes a comparison operator token if present.
    fn consume_comparison(&mut self) -> Option<ConditionalComparison> {
        let comparison = match self.peek()? {
            ConditionalToken::Equal => ConditionalComparison::Equal,
            ConditionalToken::NotEqual => ConditionalComparison::NotEqual,
            ConditionalToken::Less => ConditionalComparison::Less,
            ConditionalToken::LessEqual => ConditionalComparison::LessEqual,
            ConditionalToken::Greater => ConditionalComparison::Greater,
            ConditionalToken::GreaterEqual => ConditionalComparison::GreaterEqual,
            _ => return None,
        };
        self.position += 1;
        Some(comparison)
    }
}
