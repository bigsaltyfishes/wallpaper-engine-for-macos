//! Token-derived expression facts.

use smol_str::SmolStr;

use super::{
    AccessOperator, ArithmeticOperator, AssignmentOperator, BitwiseOperator, CommaOperator,
    ConditionalOperator, EqualityOperator, LiteralValue, LogicalOperator, OperatorType,
    PrimitiveType, RelationalOperator, TokenCursor, TokenIndexRange, TypedToken,
};

/// Reusable tokenizer-owned expression fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpressionFact {
    /// Token range covered by this expression.
    range: TokenIndexRange,
    /// Operators visible at the expression range's top level.
    top_level_operators: Vec<OperatorFact>,
    /// Segments used by modulo lowering strategies.
    modulo_lowering_segments: Vec<TokenIndexRange>,
}

impl ExpressionFact {
    /// Collects an expression fact from a token range.
    #[must_use]
    pub fn collect(tokens: TokenCursor<'_>, range: TokenIndexRange) -> Option<Self> {
        let (start, end) = tokens.non_comment_range(range.start(), range.end())?;
        let range = TokenIndexRange::from_inclusive(start, end);
        let top_level_operator_collector = TopLevelOperators { tokens, range };
        let modulo_lowering_segments = top_level_operator_collector.modulo_lowering_segments()?;
        let top_level_operators = top_level_operator_collector.collect()?;
        Some(Self {
            range,
            top_level_operators,
            modulo_lowering_segments,
        })
    }

    /// Returns this expression's token range.
    #[must_use]
    pub const fn range(&self) -> TokenIndexRange {
        self.range
    }

    /// Returns top-level operators in source order.
    #[must_use]
    pub fn top_level_operators(&self) -> &[OperatorFact] {
        &self.top_level_operators
    }

    /// Returns top-level binary operator facts matching `operators`.
    #[must_use]
    pub fn matching_top_level_operators(&self, operators: &[OperatorType]) -> Vec<OperatorFact> {
        self.top_level_operators
            .iter()
            .copied()
            .filter(|operator| operators.contains(&operator.operator()))
            .collect()
    }

    /// Splits this expression into operand ranges around the selected
    /// top-level operators.
    #[must_use]
    pub fn operand_ranges_for(&self, operators: &[OperatorFact]) -> Vec<TokenIndexRange> {
        if operators.is_empty() {
            return Vec::new();
        }
        let mut ranges = Vec::with_capacity(operators.len() + 1);
        let mut start = self.range.start();
        for operator in operators {
            if start < operator.index() {
                ranges.push(TokenIndexRange::new(start, operator.index()));
            }
            start = operator.index() + 1;
        }
        if start < self.range.end() {
            ranges.push(TokenIndexRange::new(start, self.range.end()));
        }
        ranges
    }

    /// Returns operand ranges for top-level binary operators matching
    /// `operators`.
    #[must_use]
    pub fn binary_operand_ranges_for(&self, operators: &[OperatorType]) -> Vec<TokenIndexRange> {
        let operators = self.matching_top_level_operators(operators);
        if operators.is_empty() {
            return Vec::new();
        }
        self.operand_ranges_for(&operators)
    }

    /// Splits this expression into tokenizer-owned segments for modulo
    /// lowering. Segments are separated at top-level control and lower
    /// precedence binary boundaries so strategies can lower only the reusable
    /// multiplicative spans they receive.
    #[must_use]
    pub fn modulo_lowering_segments(&self) -> &[TokenIndexRange] {
        &self.modulo_lowering_segments
    }
}

/// One typed operator visible at expression top level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperatorFact {
    /// Operator token index.
    index: usize,
    /// Typed operator.
    operator: OperatorType,
}

impl OperatorFact {
    /// Returns the operator token index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Returns the typed operator.
    #[must_use]
    pub const fn operator(self) -> OperatorType {
        self.operator
    }

    /// Returns whether this operator separates modulo lowering segments.
    #[must_use]
    const fn is_modulo_lowering_boundary(self) -> bool {
        match self.operator {
            OperatorType::Assignment(AssignmentOperator::Assign)
            | OperatorType::Conditional(
                ConditionalOperator::Question | ConditionalOperator::Colon,
            )
            | OperatorType::Relational(_)
            | OperatorType::Equality(_)
            | OperatorType::Comma(_)
            | OperatorType::Logical(_)
            | OperatorType::Bitwise(_)
            | OperatorType::Arithmetic(ArithmeticOperator::Add | ArithmeticOperator::Subtract) => {
                true
            }
            OperatorType::Assignment(_)
            | OperatorType::Arithmetic(
                ArithmeticOperator::Multiply
                | ArithmeticOperator::Divide
                | ArithmeticOperator::Remainder,
            )
            | OperatorType::Increment(_)
            | OperatorType::Access(_)
            | OperatorType::Grouping(_)
            | OperatorType::Subscript(_) => false,
        }
    }
}

/// Reusable expression shape for a token range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpressionShape {
    /// Empty or unsupported expression shape.
    Unknown,
    /// Scalar literal or identifier atom.
    ScalarAtom,
    /// Function-call-shaped expression.
    FunctionCall {
        /// Function name.
        name: SmolStr,
        /// Function name token index.
        name_index: usize,
        /// Opening parenthesis token index.
        open_index: usize,
        /// Matching closing parenthesis token index.
        close_index: usize,
        /// Top-level argument ranges.
        arguments: Vec<TokenIndexRange>,
    },
    /// Binary expression split by a typed operator.
    Binary {
        /// Operator token index.
        operator_index: usize,
        /// Typed operator.
        operator: OperatorType,
        /// Left operand range.
        left: TokenIndexRange,
        /// Right operand range.
        right: TokenIndexRange,
    },
    /// Parenthesized expression.
    Parenthesized(TokenIndexRange),
    /// Member access expression.
    Member {
        /// Base expression range.
        base: TokenIndexRange,
        /// Field name.
        field: SmolStr,
    },
    /// Index/subscript expression.
    Index {
        /// Base expression range.
        base: TokenIndexRange,
        /// Index expression range.
        index: TokenIndexRange,
    },
}

impl ExpressionShape {
    /// Classifies a half-open token range.
    #[must_use]
    pub fn classify(tokens: TokenCursor<'_>, range: TokenIndexRange) -> Self {
        let Some((start, end)) = tokens.non_comment_range(range.start(), range.end()) else {
            return Self::Unknown;
        };
        if tokens[start].kind().is_left_paren()
            && tokens[end].kind().is_right_paren()
            && tokens.matching_right_paren(start) == Some(end)
        {
            return Self::Parenthesized(TokenIndexRange::new(start + 1, end));
        }
        if let Some(binary) = Self::binary(tokens, start, end, &Self::additive_operators()) {
            return binary;
        }
        if let Some(binary) = Self::binary(tokens, start, end, &Self::multiplicative_operators()) {
            return binary;
        }
        if let TypedToken::Identifier(field) = tokens[end].kind()
            && let Some(dot) = tokens.previous_non_comment(end)
            && matches!(
                tokens[dot].kind(),
                TypedToken::Operator(OperatorType::Access(AccessOperator::Member))
            )
            && let Some(base_end) = tokens.previous_non_comment(dot)
            && start <= base_end
        {
            return Self::Member {
                base: TokenIndexRange::new(start, base_end + 1),
                field: field.clone(),
            };
        }
        if tokens[end].kind().is_right_square()
            && let Some(open) = {
                let mut depth = 0usize;
                let mut matching = None;
                for index in (start..end).rev() {
                    match tokens[index].kind() {
                        kind if kind.is_right_square() => depth += 1,
                        kind if kind.is_left_square() && depth == 0 => {
                            matching = Some(index);
                            break;
                        }
                        kind if kind.is_left_square() => {
                            let Some(updated_depth) = depth.checked_sub(1) else {
                                return Self::Unknown;
                            };
                            depth = updated_depth;
                        }
                        _ => {}
                    }
                }
                matching
            }
            && let Some(base_end) = tokens.previous_non_comment(open)
            && start <= base_end
            && let Some((index_start, index_end)) = tokens.non_comment_range(open + 1, end)
        {
            return Self::Index {
                base: TokenIndexRange::new(start, base_end + 1),
                index: TokenIndexRange::from_inclusive(index_start, index_end),
            };
        }
        if let Some(name) = tokens[start].kind().source_text()
            && let Some(open) = tokens.next_non_comment(start + 1)
            && tokens[open].kind().is_left_paren()
            && let Some(close) = tokens.matching_right_paren(open)
            && close == end
        {
            return Self::FunctionCall {
                name: name.into(),
                name_index: start,
                open_index: open,
                close_index: close,
                arguments: CallArgumentRanges::new(tokens, open, close).into_vec(),
            };
        }
        if match (
            tokens[start].kind(),
            (start < end).then(|| tokens[end].kind()),
        ) {
            (TypedToken::Literal(LiteralValue::Number(_)) | TypedToken::Identifier(_), None) => {
                true
            }
            (
                TypedToken::Operator(OperatorType::Arithmetic(
                    ArithmeticOperator::Add | ArithmeticOperator::Subtract,
                )),
                Some(TypedToken::Literal(LiteralValue::Number(_))),
            ) if start + 1 == end => true,
            _ => false,
        } {
            return Self::ScalarAtom;
        }
        Self::Unknown
    }

    /// Returns the additive operators used by scalar expression classifiers.
    #[must_use]
    pub const fn additive_operators() -> [OperatorType; 2] {
        [
            OperatorType::Arithmetic(ArithmeticOperator::Add),
            OperatorType::Arithmetic(ArithmeticOperator::Subtract),
        ]
    }

    /// Returns the multiplicative operators used by scalar expression
    /// classifiers.
    #[must_use]
    pub const fn multiplicative_operators() -> [OperatorType; 3] {
        [
            OperatorType::Arithmetic(ArithmeticOperator::Multiply),
            OperatorType::Arithmetic(ArithmeticOperator::Divide),
            OperatorType::Arithmetic(ArithmeticOperator::Remainder),
        ]
    }

    /// Returns a binary shape for the rightmost top-level matching operator.
    fn binary(
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
        operators: &[OperatorType],
    ) -> Option<Self> {
        let expression =
            ExpressionFact::collect(tokens, TokenIndexRange::from_inclusive(start, end))?;
        let operator = expression
            .matching_top_level_operators(operators)
            .into_iter()
            .next_back()?;
        Some(Self::Binary {
            operator_index: operator.index(),
            operator: operator.operator(),
            left: TokenIndexRange::new(start, operator.index()),
            right: TokenIndexRange::new(operator.index() + 1, end + 1),
        })
    }
}

/// Top-level operator collector for expression token ranges.
#[derive(Clone, Copy)]
struct TopLevelOperators<'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Expression range to inspect.
    range: TokenIndexRange,
}

impl TopLevelOperators<'_> {
    /// Collects top-level operators in `range`.
    fn collect(self) -> Option<Vec<OperatorFact>> {
        let candidates = self.collect_all()?;
        let Some(precedence) = candidates
            .iter()
            .map(|operator| Self::precedence(operator.operator()))
            .min()
        else {
            return Some(Vec::new());
        };
        Some(
            candidates
                .into_iter()
                .filter(|operator| Self::precedence(operator.operator()) == precedence)
                .collect(),
        )
    }

    /// Returns modulo-lowering segments split at all matching top-level
    /// boundaries in source order.
    fn modulo_lowering_segments(self) -> Option<Vec<TokenIndexRange>> {
        let boundaries = self
            .collect_all()?
            .into_iter()
            .filter(|operator| operator.is_modulo_lowering_boundary())
            .collect::<Vec<_>>();
        if boundaries.is_empty() {
            return Some(Vec::new());
        }
        let mut ranges = Vec::with_capacity(boundaries.len() + 1);
        let mut start = self.range.start();
        for boundary in boundaries {
            if start < boundary.index() {
                ranges.push(TokenIndexRange::new(start, boundary.index()));
            }
            start = boundary.index() + 1;
        }
        if start < self.range.end() {
            ranges.push(TokenIndexRange::new(start, self.range.end()));
        }
        Some(ranges)
    }

    /// Collects all top-level typed operator candidates in `range`.
    fn collect_all(self) -> Option<Vec<OperatorFact>> {
        let mut candidates = Vec::new();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.range.start()..self.range.end() {
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Operator(operator)
                    if paren_depth == 0
                        && bracket_depth == 0
                        && !self
                            .tokens
                            .is_unary_sign_in_range(index, self.range.start()) =>
                {
                    candidates.push(OperatorFact {
                        index,
                        operator: *operator,
                    });
                }
                TypedToken::Comma if paren_depth == 0 && bracket_depth == 0 => {
                    candidates.push(OperatorFact {
                        index,
                        operator: OperatorType::Comma(CommaOperator::Comma),
                    });
                }
                _ => {}
            }
        }
        Some(candidates)
    }

    /// Returns GLSL precedence rank where lower numbers bind more weakly.
    const fn precedence(operator: OperatorType) -> u8 {
        match operator {
            OperatorType::Comma(_) => 0,
            OperatorType::Assignment(_) => 1,
            OperatorType::Conditional(_) => 2,
            OperatorType::Logical(LogicalOperator::Or) => 3,
            OperatorType::Logical(LogicalOperator::Xor) => 4,
            OperatorType::Logical(LogicalOperator::And) => 5,
            OperatorType::Bitwise(BitwiseOperator::Or) => 6,
            OperatorType::Bitwise(BitwiseOperator::Xor) => 7,
            OperatorType::Bitwise(BitwiseOperator::And) => 8,
            OperatorType::Equality(EqualityOperator::Equal | EqualityOperator::NotEqual) => 9,
            OperatorType::Relational(
                RelationalOperator::Less
                | RelationalOperator::Greater
                | RelationalOperator::LessEqual
                | RelationalOperator::GreaterEqual,
            ) => 10,
            OperatorType::Bitwise(BitwiseOperator::ShiftLeft | BitwiseOperator::ShiftRight) => 11,
            OperatorType::Arithmetic(ArithmeticOperator::Add | ArithmeticOperator::Subtract) => 12,
            OperatorType::Arithmetic(
                ArithmeticOperator::Multiply
                | ArithmeticOperator::Divide
                | ArithmeticOperator::Remainder,
            ) => 13,
            OperatorType::Logical(LogicalOperator::Not)
            | OperatorType::Bitwise(BitwiseOperator::Not)
            | OperatorType::Increment(_) => 14,
            OperatorType::Access(_) | OperatorType::Grouping(_) | OperatorType::Subscript(_) => 15,
        }
    }
}

/// One syntactic function-call fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallFact {
    /// Function name.
    pub name: SmolStr,
    /// Function name token index.
    pub name_index: usize,
    /// Opening parenthesis token index.
    pub open_index: usize,
    /// Closing parenthesis token index.
    pub close_index: usize,
    /// Top-level argument ranges.
    pub arguments: Vec<TokenIndexRange>,
}

impl CallFact {
    /// Returns the call name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the function name token index.
    #[must_use]
    pub const fn name_index(&self) -> usize {
        self.name_index
    }

    /// Returns the opening parenthesis token index.
    #[must_use]
    pub const fn open_index(&self) -> usize {
        self.open_index
    }

    /// Returns the closing parenthesis token index.
    #[must_use]
    pub const fn close_index(&self) -> usize {
        self.close_index
    }

    /// Returns top-level argument ranges.
    #[must_use]
    pub fn arguments(&self) -> &[TokenIndexRange] {
        &self.arguments
    }
}

/// Top-level argument range collector.
#[derive(Debug)]
pub(super) struct CallArgumentRanges {
    /// Collected ranges.
    items: Vec<TokenIndexRange>,
}

impl CallArgumentRanges {
    /// Collects argument ranges between call parentheses.
    #[must_use]
    pub(super) fn new(tokens: TokenCursor<'_>, open_index: usize, close_index: usize) -> Self {
        let mut items = Vec::new();
        let mut start = open_index + 1;

        while start < close_index {
            let end = tokens.top_level_comma_segment_end(start, close_index);
            if let Some((start, end)) = tokens.non_comment_range(start, end) {
                items.push(TokenIndexRange::from_inclusive(start, end));
            }
            start = end.saturating_add(1);
        }

        Self { items }
    }

    /// Returns collected ranges.
    #[must_use]
    pub(super) fn into_vec(self) -> Vec<TokenIndexRange> {
        self.items
    }
}

/// Float constructor shapes accepted by scalar strategies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FloatConstructor;

impl FloatConstructor {
    /// Returns whether `name` is a scalar float constructor spelling.
    #[must_use]
    pub fn matches_name(name: &str) -> bool {
        name == PrimitiveType::Float.text() || name == "float1"
    }
}
