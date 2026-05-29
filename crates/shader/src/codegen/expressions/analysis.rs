//! Shared token-backed expression facts for legalizer strategies.

use smol_str::SmolStr;

use crate::{
    SourceSpan,
    syntax::{CallArgument, CallArguments},
    tokenizer::{
        AccessOperator, ArithmeticOperator, AssignmentOperator, BitwiseOperator, CallFact,
        ConditionalOperator, ExpressionShape, FloatConstructor, LiteralValue, LogicalOperator,
        OperatorType,
        OperatorType::{Access, Arithmetic, Assignment, Bitwise, Conditional, Logical, Relational},
        PrimitiveType, RelationalOperator, TokenCursor, TokenIndexRange, TypedToken,
        TypedTokenFacts,
    },
};

/// Scalar type facts used by control-flow coercion strategies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScalarType {
    /// Boolean scalar.
    Bool,
    /// Signed integer scalar.
    Int,
    /// Unsigned integer scalar.
    Uint,
    /// Floating-point scalar.
    Float,
    /// Vector whose components are floating-point scalars.
    FloatVector,
    /// Non-float or unknown non-scalar value that can shadow a scalar name.
    NonFloatAggregate,
}

impl ScalarType {
    /// Returns the scalar type for a GLSL numeric literal spelling.
    pub(crate) fn classify_numeric_literal(text: &str) -> Option<Self> {
        if text.contains(['.', 'e', 'E']) {
            Some(Self::Float)
        } else if text.ends_with(['u', 'U']) {
            Some(Self::Uint)
        } else if text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
        {
            Some(Self::Int)
        } else {
            None
        }
    }

    /// Returns whether both operands are known integer modulo operands.
    pub(crate) fn integer_modulo_operands(left: Option<Self>, right: Option<Self>) -> bool {
        matches!(
            (left, right),
            (Some(Self::Int | Self::Uint), Some(Self::Int | Self::Uint))
        )
    }

    /// Returns the integer result type for an integer binary expression.
    pub(crate) fn integer_result(left: Option<Self>, right: Option<Self>) -> Self {
        if matches!((left, right), (Some(Self::Uint), Some(Self::Uint))) {
            Self::Uint
        } else {
            Self::Int
        }
    }
}

/// Scalar facts supplied by legalizer strategies.
pub(crate) trait ScalarExpressionFacts {
    /// Returns the nearest visible declaration type for `name` at `index`.
    fn visible_type(&self, name: &str, index: usize) -> Option<ScalarType>;

    /// Returns whether `lvalue` is known to be a float scalar/component.
    fn float_lvalue(&self, _lvalue: &Lvalue) -> bool {
        false
    }

    /// Returns whether `name` is visibly known as a scalar value at `index`.
    fn scalar_identifier(&self, name: &str, index: usize) -> bool {
        self.visible_type(name, index).is_some()
    }
}

/// Scalar expression semantics selected by the strategy using the analyzer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScalarExpressionFlavor {
    /// Numeric condition coercions require same-kind integer arithmetic.
    NumericCondition,
    /// Float modulo lowering needs permissive integer operand inference.
    FloatModulo,
    /// Int initializers need to identify float-valued initializer expressions.
    IntInitializer,
    /// Reserved user `mod(float,float)` routing needs scalar-only arguments.
    ReservedModArgument,
}

/// Recursive scalar type inference for legalizer strategy expression ranges.
pub(crate) struct ScalarExpressionAnalyzer<'facts, F> {
    /// Known symbol facts.
    pub facts: &'facts F,
    /// Cached tokenizer facts for whole-call lookups.
    pub token_facts: &'facts TypedTokenFacts,
    /// Strategy-specific scalar semantics.
    pub flavor: ScalarExpressionFlavor,
}

impl<F> Clone for ScalarExpressionAnalyzer<'_, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for ScalarExpressionAnalyzer<'_, F> {}

impl<F> ScalarExpressionAnalyzer<'_, F>
where
    F: ScalarExpressionFacts,
{
    /// Returns the known scalar type for an inclusive token range.
    pub(crate) fn range_type(
        self,
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
    ) -> Option<ScalarType> {
        let (start, end) = tokens.non_comment_range(start, end + 1)?;
        if matches!(
            tokens[start].kind(),
            TypedToken::Operator(Arithmetic(
                ArithmeticOperator::Add | ArithmeticOperator::Subtract,
            ))
        ) {
            return self.range_type(tokens, start + 1, end);
        }
        match ExpressionShape::classify(tokens, TokenIndexRange::from_inclusive(start, end)) {
            ExpressionShape::Parenthesized(inner) => {
                return self.range_type(tokens, inner.start(), inner.end().saturating_sub(1));
            }
            ExpressionShape::Binary { left, right, .. } => {
                let left = self.range_type(tokens, left.start(), left.end().saturating_sub(1));
                let right = self.range_type(tokens, right.start(), right.end().saturating_sub(1));
                return self.arithmetic_result(left, right);
            }
            ExpressionShape::Unknown
            | ExpressionShape::ScalarAtom
            | ExpressionShape::FunctionCall { .. }
            | ExpressionShape::Member { .. }
            | ExpressionShape::Index { .. } => {}
        }
        if let Some(ty) = self.member_selection_type(tokens, start, end) {
            return Some(ty);
        }
        if start == end {
            return self.single_token_type(tokens, start);
        }
        self.whole_function_call_type(tokens, start, end)
    }

    /// Returns the known scalar type for a single token.
    fn single_token_type(self, tokens: TokenCursor<'_>, index: usize) -> Option<ScalarType> {
        match tokens[index].kind() {
            TypedToken::Identifier(name) => self.facts.visible_type(name, index),
            TypedToken::Literal(LiteralValue::Number(text))
                if self.flavor == ScalarExpressionFlavor::FloatModulo =>
            {
                if text.contains(['.', 'e', 'E']) {
                    Some(ScalarType::Float)
                } else if text.ends_with(['u', 'U']) {
                    Some(ScalarType::Uint)
                } else {
                    Some(ScalarType::Int)
                }
            }
            TypedToken::Literal(LiteralValue::Number(text)) => {
                ScalarType::classify_numeric_literal(text)
            }
            _ => None,
        }
    }

    /// Returns the scalar type for strategy-specific arithmetic inference.
    fn arithmetic_result(
        self,
        left: Option<ScalarType>,
        right: Option<ScalarType>,
    ) -> Option<ScalarType> {
        match self.flavor {
            ScalarExpressionFlavor::NumericCondition => {
                if matches!(left, Some(ScalarType::Float))
                    || matches!(right, Some(ScalarType::Float))
                {
                    Some(ScalarType::Float)
                } else if matches!(
                    (left, right),
                    (Some(ScalarType::Int), Some(ScalarType::Int))
                ) {
                    Some(ScalarType::Int)
                } else if matches!(
                    (left, right),
                    (Some(ScalarType::Uint), Some(ScalarType::Uint))
                ) {
                    Some(ScalarType::Uint)
                } else {
                    None
                }
            }
            ScalarExpressionFlavor::FloatModulo => {
                if matches!(left, Some(ScalarType::Float))
                    || matches!(right, Some(ScalarType::Float))
                {
                    Some(ScalarType::Float)
                } else if ScalarType::integer_modulo_operands(left, right) {
                    Some(ScalarType::integer_result(left, right))
                } else {
                    None
                }
            }
            ScalarExpressionFlavor::IntInitializer => match (left?, right?) {
                (ScalarType::Float, _) | (_, ScalarType::Float) => Some(ScalarType::Float),
                (ScalarType::Uint, _) | (_, ScalarType::Uint) => Some(ScalarType::Uint),
                _ => Some(ScalarType::Int),
            },
            ScalarExpressionFlavor::ReservedModArgument => None,
        }
    }

    /// Returns the scalar type of a supported whole-call expression.
    fn whole_function_call_type(
        self,
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
    ) -> Option<ScalarType> {
        let name = tokens[start].kind().source_text()?;
        let open = tokens.next_non_comment(start + 1)?;
        if !tokens[open].kind().is_left_paren() {
            return None;
        }
        let close = tokens.matching_right_paren(open)?;
        if close != end {
            return None;
        }
        match self.flavor {
            ScalarExpressionFlavor::FloatModulo => match name {
                "float" => Some(ScalarType::Float),
                "int" => Some(ScalarType::Int),
                "uint" => Some(ScalarType::Uint),
                _ => None,
            },
            ScalarExpressionFlavor::IntInitializer
                if matches!(
                    name,
                    "acos"
                        | "asin"
                        | "atan"
                        | "ceil"
                        | "cos"
                        | "degrees"
                        | "exp"
                        | "exp2"
                        | "floor"
                        | "fract"
                        | "fwidth"
                        | "log"
                        | "log2"
                        | "mod"
                        | "pow"
                        | "radians"
                        | "sin"
                        | "sqrt"
                        | "tan"
                        | "trunc"
                ) =>
            {
                Some(ScalarType::Float)
            }
            ScalarExpressionFlavor::NumericCondition | ScalarExpressionFlavor::IntInitializer => {
                None
            }
            ScalarExpressionFlavor::ReservedModArgument => None,
        }
    }

    /// Returns the scalar type of a supported terminal member selection.
    fn member_selection_type(
        self,
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
    ) -> Option<ScalarType> {
        let TypedToken::Identifier(field) = tokens[end].kind() else {
            return None;
        };
        let dot = tokens.previous_non_comment(end)?;
        if !matches!(
            tokens[dot].kind(),
            TypedToken::Operator(Access(AccessOperator::Member))
        ) {
            return None;
        }
        let base_end = tokens.previous_non_comment(dot)?;
        let selection = TerminalMemberSelection {
            start,
            base_start: start,
            base_end,
            field: field.clone(),
        };
        match self.flavor {
            ScalarExpressionFlavor::NumericCondition => {
                if selection.base_call(tokens, self.token_facts).is_some() {
                    return Some(ScalarType::Float);
                }
                Lvalue::ending_at(tokens, end)
                    .is_some_and(|lvalue| lvalue.has_member)
                    .then_some(ScalarType::Float)
            }
            ScalarExpressionFlavor::FloatModulo => Lvalue::ending_at(tokens, end)
                .filter(|lvalue| lvalue.has_member && self.facts.float_lvalue(lvalue))
                .map(|_lvalue| ScalarType::Float),
            ScalarExpressionFlavor::IntInitializer => {
                if !selection.field_is_float_component() {
                    return None;
                }
                let base_is_float_vector = selection.base_lvalue(tokens).is_some_and(|lvalue| {
                    lvalue.start == selection.base_start
                        && matches!(
                            self.facts.visible_type(lvalue.base.as_str(), lvalue.start),
                            Some(ScalarType::FloatVector)
                        )
                });
                (base_is_float_vector
                    || selection.base_is_float_vector_call(tokens, self.token_facts))
                .then_some(ScalarType::Float)
            }
            ScalarExpressionFlavor::ReservedModArgument => None,
        }
    }

    /// Returns whether a half-open token range is a scalar expression for
    /// strategies that need boolean scalar-vs-aggregate classification.
    pub(crate) fn is_scalar_range(self, tokens: TokenCursor<'_>, start: usize, end: usize) -> bool {
        let Some((start, end)) = tokens.non_comment_range(start, end) else {
            return false;
        };
        let mut range = TokenIndexRange::from_inclusive(start, end);
        while let ExpressionShape::Parenthesized(inner) = ExpressionShape::classify(tokens, range) {
            range = inner;
        }
        let start = range.start();
        let end = range.end();
        if self.indexed_scalar_identifier(tokens, range).is_some() {
            return true;
        }
        let Some(last) = range.last() else {
            return false;
        };
        if matches!(
            tokens[start].kind(),
            TypedToken::Operator(Arithmetic(
                ArithmeticOperator::Add | ArithmeticOperator::Subtract,
            ))
        ) {
            return self.is_scalar_range(tokens, start + 1, end);
        }
        match ExpressionShape::classify(tokens, range) {
            ExpressionShape::ScalarAtom => self.scalar_atom(tokens, start, end),
            ExpressionShape::FunctionCall {
                name,
                open_index,
                close_index,
                ..
            } if FloatConstructor::matches_name(name.as_str()) && close_index == last => {
                self.is_scalar_range(tokens, open_index + 1, close_index)
            }
            ExpressionShape::Binary { left, right, .. } => {
                self.is_scalar_range(tokens, left.start(), left.end())
                    && self.is_scalar_range(tokens, right.start(), right.end())
            }
            ExpressionShape::Unknown if self.indexed_scalar_identifier(tokens, range).is_some() => {
                true
            }
            ExpressionShape::Unknown
            | ExpressionShape::FunctionCall { .. }
            | ExpressionShape::Parenthesized(_)
            | ExpressionShape::Member { .. }
            | ExpressionShape::Index { .. } => false,
        }
    }

    /// Returns whether the subrange is a scalar literal or known scalar name.
    fn scalar_atom(self, tokens: TokenCursor<'_>, start: usize, end: usize) -> bool {
        let meaningful = (start..end).collect::<Vec<_>>();
        match meaningful.as_slice() {
            [index] => {
                SignedNumber::parse(tokens[*index].kind(), None).is_some()
                    || matches!(
                        tokens[*index].kind(),
                        TypedToken::Identifier(name)
                            if self.facts.scalar_identifier(name, *index)
                    )
            }
            [sign, number]
                if SignedNumber::parse(tokens[*sign].kind(), Some(tokens[*number].kind()))
                    .is_some() =>
            {
                true
            }
            _ => false,
        }
    }

    /// Returns the base identifier index for a known scalar array element
    /// access.
    fn indexed_scalar_identifier(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Option<usize> {
        let (start, end) = tokens.non_comment_range(range.start(), range.end())?;
        let TypedToken::Identifier(name) = tokens[start].kind() else {
            return None;
        };
        let open = tokens.next_non_comment(start + 1)?;
        let close = tokens.matching_right_square(open)?;
        (tokens[open].kind().is_left_square()
            && close == end
            && self.facts.scalar_identifier(name, start))
        .then_some(start)
    }
}

/// Optionally signed numeric literal token sequence.
struct SignedNumber;

impl SignedNumber {
    /// Creates a number marker for number or sign-plus-number token kinds.
    fn parse(first: &TypedToken, second: Option<&TypedToken>) -> Option<Self> {
        match (first, second) {
            (TypedToken::Literal(LiteralValue::Number(_)), None)
            | (
                TypedToken::Operator(Arithmetic(
                    ArithmeticOperator::Add | ArithmeticOperator::Subtract,
                )),
                Some(TypedToken::Literal(LiteralValue::Number(_))),
            ) => Some(Self),
            _ => None,
        }
    }
}

/// Token-backed assignable expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lvalue {
    /// First token in the lvalue.
    pub start: usize,
    /// Last token in the lvalue.
    pub end: usize,
    /// Base identifier name.
    pub base: SmolStr,
    /// Whether the lvalue selects a scalar member such as `color.x`.
    pub has_member: bool,
}

impl Lvalue {
    /// Finds a simple identifier/member/index lvalue ending at `end`.
    pub(crate) fn ending_at(tokens: TokenCursor<'_>, end: usize) -> Option<Self> {
        let (mut start, mut base, mut has_member) = match tokens[end].kind() {
            TypedToken::Identifier(name) => (end, name.clone(), false),
            kind if kind.is_right_square() => {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..=end).rev() {
                    match tokens[index].kind() {
                        kind if kind.is_right_square() => depth += 1,
                        kind if kind.is_left_square() => {
                            depth = depth.checked_sub(1)?;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let base_end = tokens.previous_non_comment(open?)?;
                let lvalue = Self::ending_at(tokens, base_end)?;
                (lvalue.start, lvalue.base, lvalue.has_member)
            }
            _ => return None,
        };

        while let Some(dot) = tokens.previous_non_comment(start) {
            if !matches!(
                tokens[dot].kind(),
                TypedToken::Operator(Access(AccessOperator::Member))
            ) {
                break;
            }
            let base_end = tokens.previous_non_comment(dot)?;
            let lvalue = Self::ending_at(tokens, base_end)?;
            start = lvalue.start;
            base = lvalue.base;
            has_member = true;
        }

        Some(Self {
            start,
            end,
            base,
            has_member,
        })
    }
}

/// Terminal member selection facts for an expression range.
#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalMemberSelection {
    /// First token in the whole expression.
    start: usize,
    /// First token in the selected base expression.
    base_start: usize,
    /// Last token in the selected base expression.
    base_end: usize,
    /// Field identifier text.
    field: SmolStr,
}

impl TerminalMemberSelection {
    /// Returns whether the terminal field is a float-vector component selector.
    fn field_is_float_component(&self) -> bool {
        !self.field.is_empty()
            && self.field.bytes().all(|component| {
                matches!(
                    component,
                    b'x' | b'y' | b'z' | b'w' | b'r' | b'g' | b'b' | b'a'
                )
            })
    }

    /// Returns the lvalue used as the member base.
    fn base_lvalue(&self, tokens: TokenCursor<'_>) -> Option<Lvalue> {
        Lvalue::ending_at(tokens, self.base_end)
    }

    /// Returns whether the base is exactly a whole function call.
    fn base_call(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
    ) -> Option<FunctionCallRange> {
        (self.base_start >= self.start)
            .then_some(token_facts.call_at_name(self.base_start))
            .flatten()
            .map(|fact| FunctionCallRange::from_fact(tokens, fact))
            .filter(|call| call.close_index == self.base_end)
    }

    /// Returns whether the member base is a call known to return a float
    /// vector.
    fn base_is_float_vector_call(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
    ) -> bool {
        self.base_call(tokens, token_facts).is_some_and(|call| {
            call.name_index == self.base_start
                && matches!(
                    call.name.as_str(),
                    "texture"
                        | "texture2D"
                        | "textureLod"
                        | "texture2DLod"
                        | "texSample2D"
                        | "texSample2DLod"
                )
        })
    }
}

/// Function call token facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionCallRange {
    /// Function name.
    pub name: SmolStr,
    /// Function name token index.
    pub name_index: usize,
    /// Opening parenthesis token index.
    pub open_index: usize,
    /// Matching closing parenthesis token index.
    pub close_index: usize,
    /// Top-level call arguments.
    pub arguments: CallArguments,
}

impl FunctionCallRange {
    /// Creates a legalizer call range from an already-collected call fact.
    fn from_fact(tokens: TokenCursor<'_>, fact: &CallFact) -> Self {
        Self {
            name: SmolStr::new(fact.name()),
            name_index: fact.name_index(),
            open_index: fact.open_index(),
            close_index: fact.close_index(),
            arguments: CallArguments::from_ranges(tokens, fact.arguments()),
        }
    }
}

/// Supported vector widths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VectorWidth {
    /// `vec2`.
    Two,
    /// `vec3`.
    Three,
    /// `vec4`.
    Four,
}

impl VectorWidth {
    /// Classifies a vector constructor/type name.
    pub(crate) const fn classify_constructor(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"vec2" | b"float2" => Some(Self::Two),
            b"vec3" | b"float3" => Some(Self::Three),
            b"vec4" | b"float4" => Some(Self::Four),
            _ => None,
        }
    }

    /// Returns GLSL constructor spelling.
    pub(crate) const fn constructor(self) -> &'static str {
        match self {
            Self::Two => "vec2",
            Self::Three => "vec3",
            Self::Four => "vec4",
        }
    }

    /// Returns component swizzle needed to narrow from vec4.
    pub(crate) const fn narrow_swizzle(self) -> Option<&'static str> {
        match self {
            Self::Two => Some(".xy"),
            Self::Three => Some(".xyz"),
            Self::Four => None,
        }
    }

    /// Returns a swizzle that selects this many components.
    pub(crate) const fn component_swizzle(self) -> &'static str {
        match self {
            Self::Two => ".xy",
            Self::Three => ".xyz",
            Self::Four => ".xyzw",
        }
    }

    /// Returns the number of vector components.
    pub(crate) const fn component_count(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}

impl PartialOrd for VectorWidth {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VectorWidth {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.component_count().cmp(&other.component_count())
    }
}

/// Vector swizzle field on a member access expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SwizzleField {
    /// Width implied by the field component count.
    pub width: VectorWidth,
}

impl SwizzleField {
    /// Parses a vector swizzle field from component text.
    pub(crate) fn parse(field: impl AsRef<str>) -> Result<Self, ()> {
        let field = field.as_ref();
        let width = if !(2..=4).contains(&field.len())
            || !field.bytes().all(|byte| {
                matches!(
                    byte,
                    b'x' | b'y'
                        | b'z'
                        | b'w'
                        | b'r'
                        | b'g'
                        | b'b'
                        | b'a'
                        | b's'
                        | b't'
                        | b'p'
                        | b'q'
                )
            }) {
            None
        } else {
            match field.len() {
                2 => Some(VectorWidth::Two),
                3 => Some(VectorWidth::Three),
                4 => Some(VectorWidth::Four),
                _ => None,
            }
        }
        .ok_or(())?;
        Ok(Self { width })
    }
}

/// Vector facts supplied by legalizer strategies.
pub(crate) trait VectorExpressionFacts {
    /// Returns the scalar/vector expression type for an identifier.
    fn expression_type(&self, name: &str, index: usize) -> Option<VectorExpressionType>;

    /// Returns the vector width of an identifier expression.
    fn vector_width(&self, name: &str, index: usize) -> Option<VectorWidth> {
        match self.expression_type(name, index) {
            Some(VectorExpressionType::Vector(width)) => Some(width),
            Some(VectorExpressionType::Scalar | VectorExpressionType::Blocker) | None => None,
        }
    }

    /// Returns the declared type of a function call expression.
    fn function_return(
        &self,
        _tokens: TokenCursor<'_>,
        _token_facts: &TypedTokenFacts,
        _call: &FunctionCallRange,
    ) -> Option<VectorExpressionType> {
        None
    }
}

/// Scalar/vector shape for one visible expression fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VectorExpressionType {
    /// Scalar expression.
    Scalar,
    /// Vector expression width.
    Vector(VectorWidth),
    /// Aggregate that blocks scalar/vector inference.
    Blocker,
}

/// Shared vector expression facts for type coercion strategies.
pub(crate) struct VectorExpressionAnalyzer<'facts, F> {
    /// Known vector facts.
    pub facts: &'facts F,
    /// Cached tokenizer facts for whole-call lookups.
    pub token_facts: &'facts TypedTokenFacts,
}

impl<F> Clone for VectorExpressionAnalyzer<'_, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for VectorExpressionAnalyzer<'_, F> {}

impl<F> VectorExpressionAnalyzer<'_, F>
where
    F: VectorExpressionFacts,
{
    /// Returns the vector width produced by a simple operand.
    pub(crate) fn simple_width(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Option<VectorWidth> {
        let (start, end) = tokens.non_comment_range(range.start(), range.end())?;
        if start == end {
            let TypedToken::Identifier(name) = tokens[start].kind() else {
                return None;
            };
            return self.facts.vector_width(name, start);
        }
        let swizzled = Self::terminal_swizzle(tokens, TokenIndexRange::from_inclusive(start, end))?;
        self.facts
            .vector_width(swizzled.base_name.as_str(), swizzled.base_index)
            .map(|_width| swizzled.width)
    }

    /// Returns the vector width produced by a call argument expression.
    pub(crate) fn argument_vector_width(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<VectorWidth> {
        if let Some(inner) = Self::parenthesized_argument(tokens, argument) {
            return self.argument_vector_width(tokens, inner);
        }
        if argument.is_single_token() {
            let TypedToken::Identifier(name) = tokens[argument.start()].kind() else {
                return None;
            };
            return self.facts.vector_width(name, argument.start());
        }
        if let Some(swizzled) = self.terminal_swizzle_for(
            tokens,
            TokenIndexRange::from_inclusive(argument.start(), argument.end()),
        ) {
            return Some(swizzled.width);
        }
        let name = tokens[argument.start()].kind().source_text()?;
        if let Ok(function) = VectorReturningFunction::classify(name) {
            let open = tokens.next_non_comment(argument.start() + 1)?;
            if !tokens[open].kind().is_left_paren()
                || !tokens[argument.end()].kind().is_right_paren()
                || tokens.matching_right_paren(open) != Some(argument.end())
            {
                return None;
            }
            let call = self.whole_argument_call(tokens, argument)?;
            return call
                .arguments
                .iter()
                .enumerate()
                .filter(|(index, _argument)| function.selects_argument(*index))
                .filter_map(|(_index, argument)| self.argument_vector_width(tokens, argument))
                .min();
        }
        if let Some(width) = self.call_return_width(tokens, argument) {
            return Some(width);
        }
        if let Some(width) = self.binary_vector_width(tokens, argument) {
            return Some(width);
        }
        let width = VectorWidth::classify_constructor(name)?;
        (tokens[argument.start() + 1].kind().is_left_paren()
            && tokens[argument.end()].kind().is_right_paren())
        .then_some(width)
    }

    /// Returns whether the argument is safe to treat as a scalar expression.
    pub(crate) fn argument_is_scalar_like(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> bool {
        if argument.is_single_token() {
            return self.single_token_scalar_like(tokens, argument.start());
        }

        Self::parenthesized_argument(tokens, argument)
            .is_some_and(|inner| self.argument_is_scalar_like(tokens, inner))
            || self.is_scalar_call(tokens, argument)
            || self.is_scalar_expression(tokens, argument)
    }

    /// Returns whether a swizzle must be applied to a parenthesized
    /// expression.
    pub(crate) fn needs_parentheses_for_swizzle(argument: CallArgument) -> bool {
        !argument.is_single_token()
    }

    /// Returns vector operands that should be narrowed for sampler2D
    /// coordinate expressions.
    pub(crate) fn sampler2d_coordinate_operands(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Vec<VectorCoordinateOperand> {
        self.coordinate_range_operands(
            tokens,
            TokenIndexRange::from_inclusive(argument.start(), argument.end()),
        )
    }

    /// Parses a simple `identifier.swizzle` expression from token bounds.
    pub(crate) fn terminal_swizzle(
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Option<TerminalVectorSwizzle> {
        let (start, end) = tokens.non_comment_range(range.start(), range.end())?;
        let TypedToken::Identifier(field) = tokens[end].kind() else {
            return None;
        };
        let field = SwizzleField::parse(field).ok()?;
        let dot = tokens.previous_non_comment(end)?;
        if !matches!(
            tokens[dot].kind(),
            TypedToken::Operator(Access(AccessOperator::Member))
        ) {
            return None;
        }
        let base_index = tokens.previous_non_comment(dot)?;
        if base_index != start {
            return None;
        }
        let TypedToken::Identifier(base_name) = tokens[base_index].kind() else {
            return None;
        };
        Some(TerminalVectorSwizzle {
            base_index,
            base_name: base_name.clone(),
            width: field.width,
        })
    }

    /// Parses a simple `identifier.swizzle` expression whose base is a known
    /// vector expression.
    pub(crate) fn terminal_swizzle_for(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Option<TerminalVectorSwizzle> {
        let swizzled = Self::terminal_swizzle(tokens, range)?;
        self.facts
            .vector_width(swizzled.base_name.as_str(), swizzled.base_index)
            .map(|_width| swizzled)
    }

    /// Parses a source span for a vector constructor operand starting at
    /// `start`.
    pub(crate) fn constructor_operand_span(
        tokens: TokenCursor<'_>,
        start: usize,
        width: VectorWidth,
    ) -> Option<SourceSpan> {
        let name = tokens[start].kind().source_text()?;
        if VectorWidth::classify_constructor(name) != Some(width)
            && !(name == "CAST2" && width == VectorWidth::Two)
        {
            return None;
        }
        let open = tokens.next_non_comment(start + 1)?;
        if !tokens[open].kind().is_left_paren() {
            return None;
        }
        let close = tokens.matching_right_paren(open)?;
        SourceSpan::new(tokens[start].span().start(), tokens[close].span().end()).ok()
    }

    /// Returns trailing swizzles for wider binary operands in `range`.
    pub(crate) fn binary_operand_swizzles(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
        target_width: VectorWidth,
    ) -> Vec<VectorOperandSwizzle> {
        let operands = self.token_facts.binary_expression_operand_ranges(
            tokens,
            range,
            &Self::arithmetic_operators(),
        );
        let swizzle_operands = operands
            .iter()
            .filter_map(|operand| {
                let width = self.simple_width(tokens, *operand)?;
                (width > target_width).then_some((*operand, width))
            })
            .collect::<Vec<_>>();
        if swizzle_operands.is_empty()
            || !operands
                .iter()
                .any(|operand| self.simple_width(tokens, *operand) == Some(target_width))
        {
            return Vec::new();
        }

        swizzle_operands
            .into_iter()
            .map(|(operand, _width)| VectorOperandSwizzle {
                insertion: SourceSpan::new(
                    tokens[operand.end() - 1].span().end(),
                    tokens[operand.end() - 1].span().end(),
                )
                .unwrap_or(tokens[operand.end() - 1].span()),
                swizzle: target_width.component_swizzle(),
            })
            .collect()
    }

    /// Returns arithmetic operators used by vector binary strategies.
    const fn arithmetic_operators() -> [OperatorType; 5] {
        [
            Arithmetic(ArithmeticOperator::Add),
            Arithmetic(ArithmeticOperator::Subtract),
            Arithmetic(ArithmeticOperator::Multiply),
            Arithmetic(ArithmeticOperator::Divide),
            Arithmetic(ArithmeticOperator::Remainder),
        ]
    }

    /// Returns whether an expression's top-level operators are scalar-only and
    /// every identifier operand is known scalar or unresolved.
    fn is_scalar_expression(self, tokens: TokenCursor<'_>, argument: CallArgument) -> bool {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut saw_scalar_operator = false;
        let mut index = argument.start();

        while index <= argument.end() {
            match tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TypedToken::Identifier(name) => {
                    if let Some(close) = Self::scalar_call_end(tokens, index) {
                        if !self.call_result_scalar_like(tokens, index, close) {
                            return false;
                        }
                        index = close + 1;
                        continue;
                    }
                    if !self.identifier_scalar_like(tokens, index, name, paren_depth, bracket_depth)
                    {
                        return false;
                    }
                }
                TypedToken::TypeMark(
                    PrimitiveType::Float | PrimitiveType::Int | PrimitiveType::Uint,
                ) => {
                    if let Some(close) = Self::scalar_call_end(tokens, index) {
                        if !self.call_result_scalar_like(tokens, index, close) {
                            return false;
                        }
                        index = close + 1;
                        continue;
                    }
                    return false;
                }
                TypedToken::Operator(
                    Access(AccessOperator::Member)
                    | Conditional(ConditionalOperator::Question | ConditionalOperator::Colon),
                ) => return false,
                TypedToken::Operator(
                    Relational(RelationalOperator::Less | RelationalOperator::Greater)
                    | Assignment(AssignmentOperator::Assign)
                    | Bitwise(BitwiseOperator::And | BitwiseOperator::Or | BitwiseOperator::Xor)
                    | Logical(LogicalOperator::Not),
                ) if paren_depth == 0 && bracket_depth == 0 => {
                    return false;
                }
                TypedToken::Operator(Arithmetic(
                    ArithmeticOperator::Add
                    | ArithmeticOperator::Subtract
                    | ArithmeticOperator::Multiply
                    | ArithmeticOperator::Divide
                    | ArithmeticOperator::Remainder,
                )) if paren_depth == 0 && bracket_depth == 0 => {
                    saw_scalar_operator = true;
                }
                _ => {}
            }
            index += 1;
        }

        saw_scalar_operator
    }

    /// Returns whether a one-token expression is known scalar-like.
    fn single_token_scalar_like(self, tokens: TokenCursor<'_>, index: usize) -> bool {
        let TypedToken::Identifier(name) = tokens[index].kind() else {
            return matches!(
                tokens[index].kind(),
                TypedToken::Literal(LiteralValue::Number(_))
            );
        };
        self.identifier_scalar_like(tokens, index, name, 0, 0)
    }

    /// Returns whether an identifier reference is known scalar-like in this
    /// argument.
    fn identifier_scalar_like(
        self,
        tokens: TokenCursor<'_>,
        index: usize,
        name: &str,
        paren_depth: usize,
        bracket_depth: usize,
    ) -> bool {
        if tokens
            .next_non_comment(index + 1)
            .is_some_and(|next| tokens[next].kind().is_left_paren())
            && (paren_depth > 0 || bracket_depth == 0)
        {
            return true;
        }
        matches!(
            self.facts.expression_type(name, index),
            Some(VectorExpressionType::Scalar) | None
        )
    }

    /// Returns whether a nested call has typed scalar return evidence.
    fn call_result_scalar_like(self, tokens: TokenCursor<'_>, start: usize, close: usize) -> bool {
        let Some(name) = tokens[start].kind().source_text() else {
            return false;
        };
        if VectorReturningFunction::classify(name).is_ok() {
            return false;
        }
        let Some(argument) = CallArgument::trim_from_bounds(tokens, start, close + 1) else {
            return false;
        };
        if self.argument_vector_width(tokens, argument).is_some() {
            return false;
        }
        self.call_return_type(tokens, argument)
            .is_some_and(|ty| ty == VectorExpressionType::Scalar)
            || BuiltinScalarReturningFunction::NAMES.contains(&name)
    }

    /// Returns the closing parenthesis for a nested call that can be treated as
    /// a scalar expression leaf.
    fn scalar_call_end(tokens: TokenCursor<'_>, index: usize) -> Option<usize> {
        let name = tokens[index].kind().source_text()?;
        if VectorWidth::classify_constructor(name).is_some() {
            return None;
        }
        let open = tokens.next_non_comment(index + 1)?;
        if !tokens[open].kind().is_left_paren() {
            return None;
        }
        tokens.matching_right_paren(open)
    }

    /// Returns whether the whole expression is a scalar-compatible call.
    fn is_scalar_call(self, tokens: TokenCursor<'_>, argument: CallArgument) -> bool {
        self.argument_vector_width(tokens, argument).is_none()
            && self.call_return_type(tokens, argument).is_some_and(|ty| {
                ty == VectorExpressionType::Scalar
                    && Self::scalar_call_end(tokens, argument.start())
                        .is_some_and(|close| close == argument.end())
            })
    }

    /// Returns a parenthesized inner expression if the whole argument is
    /// wrapped in one balanced pair.
    fn parenthesized_argument(
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<CallArgument> {
        if !tokens[argument.start()].kind().is_left_paren()
            || !tokens[argument.end()].kind().is_right_paren()
        {
            return None;
        }
        let close = tokens.matching_right_paren(argument.start())?;
        if close != argument.end() {
            return None;
        }
        CallArgument::trim_from_bounds(tokens, argument.start() + 1, argument.end())
    }

    /// Returns the declared return type for a whole-call argument.
    pub(crate) fn call_return_type(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<VectorExpressionType> {
        let TypedToken::Identifier(name) = tokens[argument.start()].kind() else {
            return None;
        };
        if VectorWidth::classify_constructor(name).is_some() {
            return None;
        }
        let open = tokens.next_non_comment(argument.start() + 1)?;
        if !tokens[open].kind().is_left_paren() {
            return None;
        }
        let close = tokens.matching_right_paren(open)?;
        if close != argument.end() {
            return None;
        }
        let call = self.whole_argument_call(tokens, argument)?;
        self.facts.function_return(tokens, self.token_facts, &call)
    }

    /// Returns the declared vector return width for a whole-call argument.
    pub(crate) fn call_return_width(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<VectorWidth> {
        match self.call_return_type(tokens, argument)? {
            VectorExpressionType::Vector(width) => Some(width),
            VectorExpressionType::Scalar | VectorExpressionType::Blocker => None,
        }
    }

    /// Returns cached call facts when the whole argument is exactly one call.
    fn whole_argument_call(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<FunctionCallRange> {
        let call =
            FunctionCallRange::from_fact(tokens, self.token_facts.call_at_name(argument.start())?);
        (call.close_index == argument.end()).then_some(call)
    }

    /// Returns vector width implied by top-level vector binary operands.
    fn binary_vector_width(
        self,
        tokens: TokenCursor<'_>,
        argument: CallArgument,
    ) -> Option<VectorWidth> {
        let expression = self
            .token_facts
            .expression_covering(argument.start()..argument.end() + 1)?;
        let operators = expression.matching_top_level_operators(&Self::arithmetic_operators());
        if operators.is_empty() {
            return None;
        }
        let operands = expression.operand_ranges_for(&operators);
        operands
            .into_iter()
            .map(|range| CallArgument::trim_from_bounds(tokens, range.start(), range.end()))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .filter_map(|argument| self.argument_vector_width(tokens, argument))
            .try_fold(None, |selected, width| match selected {
                Some(selected) if selected != width => None,
                Some(selected) => Some(Some(selected)),
                None => Some(Some(width)),
            })
            .flatten()
    }

    /// Returns vector operands for one coordinate range.
    fn coordinate_range_operands(
        self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
    ) -> Vec<VectorCoordinateOperand> {
        let Some(argument) = CallArgument::trim_from_bounds(tokens, range.start(), range.end())
        else {
            return Vec::new();
        };
        let expression = self
            .token_facts
            .expression_covering(argument.start()..argument.end() + 1);
        let binary_operand_ranges = if let Some(expression) = expression {
            let operators = expression.matching_top_level_operators(&Self::arithmetic_operators());
            if operators.is_empty() {
                Vec::new()
            } else {
                expression.operand_ranges_for(&operators)
            }
        } else {
            Vec::new()
        };
        let binary_operands = binary_operand_ranges
            .into_iter()
            .flat_map(|range| self.coordinate_range_operands(tokens, range))
            .collect::<Vec<_>>();
        if !binary_operands.is_empty() {
            return binary_operands;
        }
        let parenthesized_operands = if let Some(inner) =
            Self::parenthesized_argument(tokens, argument)
            && inner.start() == range.start() + 1
            && inner.end() + 1 == range.end() - 1
        {
            self.sampler2d_coordinate_operands(tokens, inner)
        } else {
            Vec::new()
        };
        if !parenthesized_operands.is_empty() {
            return parenthesized_operands;
        }
        if let Some(VectorWidth::Three | VectorWidth::Four) =
            self.argument_vector_width(tokens, argument)
        {
            return vec![VectorCoordinateOperand {
                span: argument.span(),
                wrap: Self::needs_parentheses_for_swizzle(argument),
            }];
        }
        Vec::new()
    }
}

/// Builtin whose result keeps the width of one or more vector arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VectorReturningFunction {
    /// One-argument component-wise numeric builtin.
    Unary,
    /// Two-argument component-wise numeric builtin.
    ComponentWise,
    /// `mix` result shape follows the first two arguments.
    Mix,
    /// `step` result shape follows the sampled value argument.
    Step,
    /// `smoothstep` result shape follows the sampled value argument.
    Smoothstep,
    /// Ternary clamp-like builtin.
    Clamp,
}

impl VectorReturningFunction {
    /// Classifies a builtin name by vector return behavior.
    fn classify(name: &str) -> Result<Self, ()> {
        let function = match name.as_bytes() {
            b"abs" | b"acos" | b"asin" | b"atan" | b"ceil" | b"cos" | b"exp" | b"exp2"
            | b"floor" | b"fract" | b"frac" | b"inversesqrt" | b"log" | b"log2" | b"normalize"
            | b"round" | b"sign" | b"sin" | b"sqrt" | b"tan" | b"trunc" => Self::Unary,
            b"max" | b"min" | b"pow" => Self::ComponentWise,
            b"mix" => Self::Mix,
            b"step" => Self::Step,
            b"smoothstep" => Self::Smoothstep,
            b"clamp" => Self::Clamp,
            _ => return Err(()),
        };
        Ok(function)
    }

    /// Returns whether argument `index` contributes to the result width.
    const fn selects_argument(self, index: usize) -> bool {
        match self {
            Self::Unary => index == 0,
            Self::ComponentWise => true,
            Self::Mix => index <= 1,
            Self::Step => index == 1,
            Self::Smoothstep => index == 2,
            Self::Clamp => index <= 2,
        }
    }
}

/// Builtin scalar-return predicates for scalar-like argument classification.
struct BuiltinScalarReturningFunction;

impl BuiltinScalarReturningFunction {
    /// Builtins that return a scalar regardless of unknown argument widths.
    const NAMES: &'static [&'static str] = &["dot", "length", "distance", "textureQueryLevels"];
}

/// Coordinate operand that must be narrowed before sampler2D use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VectorCoordinateOperand {
    /// Operand source span.
    pub span: SourceSpan,
    /// Whether the operand must be wrapped before appending `.xy`.
    pub wrap: bool,
}

/// Terminal vector swizzle facts for a simple expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalVectorSwizzle {
    /// Base identifier token index.
    pub base_index: usize,
    /// Base identifier text.
    pub base_name: SmolStr,
    /// Width produced by the final swizzle.
    pub width: VectorWidth,
}

/// One wide vector binary operand that needs a trailing swizzle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VectorOperandSwizzle {
    /// Insertion point immediately after the operand.
    pub insertion: SourceSpan,
    /// Swizzle text to insert.
    pub swizzle: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::TokenStream;

    #[test]
    fn scalar_type_classifies_numeric_literals() {
        assert_eq!(
            ScalarType::classify_numeric_literal("1"),
            Some(ScalarType::Int)
        );
        assert_eq!(
            ScalarType::classify_numeric_literal("1u"),
            Some(ScalarType::Uint)
        );
        assert_eq!(
            ScalarType::classify_numeric_literal("1.0"),
            Some(ScalarType::Float)
        );
        assert_eq!(ScalarType::classify_numeric_literal("@"), None);
    }

    #[test]
    fn vector_expression_splits_swizzled_binary_operands() {
        #[derive(Clone, Copy)]
        struct Facts;

        impl VectorExpressionFacts for Facts {
            fn expression_type(&self, name: &str, _index: usize) -> Option<VectorExpressionType> {
                match name {
                    "uv" => Some(VectorExpressionType::Vector(VectorWidth::Two)),
                    "color" => Some(VectorExpressionType::Vector(VectorWidth::Four)),
                    _ => None,
                }
            }
        }

        let tokens = TokenStream::lex("uv + color.rgba").expect("tokens lex");
        let facts = tokens.facts();
        let cursor = tokens.cursor();
        let analyzer = VectorExpressionAnalyzer {
            facts: &Facts,
            token_facts: &facts,
        };
        let range = TokenIndexRange::new(0, cursor.len());

        let swizzles = analyzer.binary_operand_swizzles(cursor, range, VectorWidth::Two);

        assert_eq!(swizzles.len(), 1);
        assert_eq!(swizzles[0].swizzle, ".xy");
    }

    #[test]
    fn vector_expression_uses_cached_call_facts_for_nested_vector_call_width() {
        #[derive(Clone, Copy)]
        struct Facts;

        impl VectorExpressionFacts for Facts {
            fn expression_type(&self, name: &str, _index: usize) -> Option<VectorExpressionType> {
                match name {
                    "coord" => Some(VectorExpressionType::Vector(VectorWidth::Two)),
                    _ => None,
                }
            }
        }

        let tokens = TokenStream::lex("max(1.0, abs(coord))").expect("tokens lex");
        let facts = tokens.facts();
        let cursor = tokens.cursor();
        let analyzer = VectorExpressionAnalyzer {
            facts: &Facts,
            token_facts: &facts,
        };
        let call = facts.call_at_name(0).expect("outer call fact exists");
        let argument = CallArguments::from_ranges(cursor, call.arguments())
            .get(1)
            .expect("argument exists");

        let width = analyzer.argument_vector_width(cursor, argument);

        assert_eq!(width, Some(VectorWidth::Two));
    }

    #[test]
    fn scalar_expression_accepts_indexed_scalar_identifier() {
        #[derive(Clone, Copy)]
        struct Facts;

        impl ScalarExpressionFacts for Facts {
            fn visible_type(&self, name: &str, _index: usize) -> Option<ScalarType> {
                (name == "values").then_some(ScalarType::Float)
            }
        }

        let tokens = TokenStream::lex("values[0]").expect("tokens lex");
        let facts = tokens.facts();
        let cursor = tokens.cursor();
        let analyzer = ScalarExpressionAnalyzer {
            facts: &Facts,
            token_facts: &facts,
            flavor: ScalarExpressionFlavor::ReservedModArgument,
        };

        assert!(analyzer.is_scalar_range(cursor, 0, cursor.len()));
    }
}
