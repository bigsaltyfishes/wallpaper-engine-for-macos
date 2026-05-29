//! Lifetime-free typed shader tokens.

use smol_str::SmolStr;

use crate::SourceSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    /// Typed token payload.
    kind: TypedToken,
    /// Byte span in the original source.
    span: SourceSpan,
}

impl Token {
    #[must_use]
    pub const fn new(kind: TypedToken, span: SourceSpan) -> Self {
        Self { kind, span }
    }

    #[must_use]
    pub const fn kind(&self) -> &TypedToken {
        &self.kind
    }

    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedToken {
    Annotation(SmolStr),
    Directive(SmolStr),
    Identifier(SmolStr),
    Keyword(KeywordType),
    TypeMark(PrimitiveType),
    Literal(LiteralValue),
    StringLiteral(SmolStr),
    Other(SmolStr),
    LeftBrace,
    RightBrace,
    Semicolon,
    Comma,
    Operator(OperatorType),
    Eof,
}

impl TypedToken {
    #[must_use]
    pub fn from_identifier_text(text: &str) -> Self {
        if let Some(keyword) = KeywordType::parse(text) {
            Self::Keyword(keyword)
        } else if let Some(primitive) = PrimitiveType::parse(text) {
            Self::TypeMark(primitive)
        } else {
            Self::Identifier(SmolStr::new(text))
        }
    }

    #[must_use]
    pub fn identifier_text(&self) -> Option<&str> {
        match self {
            Self::Identifier(text) => Some(text.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_keyword(&self, keyword: KeywordType) -> bool {
        matches!(self, Self::Keyword(found) if *found == keyword)
    }

    #[must_use]
    pub fn is_operator(&self, operator: OperatorType) -> bool {
        matches!(self, Self::Operator(found) if *found == operator)
    }

    #[must_use]
    pub fn is_simple_assignment_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_simple_assignment())
    }

    #[must_use]
    pub fn is_compound_assignment_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_compound_assignment())
    }

    #[must_use]
    pub fn is_assignment_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_assignment())
    }

    #[must_use]
    pub fn is_member_access_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_member_access())
    }

    #[must_use]
    pub fn is_additive_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_additive())
    }

    #[must_use]
    pub fn is_multiplicative_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_multiplicative())
    }

    #[must_use]
    pub fn is_scalar_binary_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_scalar_binary())
    }

    #[must_use]
    pub fn is_comparison_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_comparison())
    }

    #[must_use]
    pub fn is_logical_not_operator(&self) -> bool {
        matches!(self, Self::Operator(operator) if operator.is_logical_not())
    }

    #[must_use]
    pub fn is_increment_operator(&self) -> bool {
        matches!(self, Self::Operator(OperatorType::Increment(_)))
    }

    #[must_use]
    pub fn is_left_paren(&self) -> bool {
        matches!(
            self,
            Self::Operator(OperatorType::Grouping(GroupingOperator::LeftParen))
        )
    }

    #[must_use]
    pub fn is_right_paren(&self) -> bool {
        matches!(
            self,
            Self::Operator(OperatorType::Grouping(GroupingOperator::RightParen))
        )
    }

    #[must_use]
    pub fn is_left_square(&self) -> bool {
        matches!(
            self,
            Self::Operator(OperatorType::Subscript(SubscriptOperator::LeftSquare))
        )
    }

    #[must_use]
    pub fn is_right_square(&self) -> bool {
        matches!(
            self,
            Self::Operator(OperatorType::Subscript(SubscriptOperator::RightSquare))
        )
    }

    #[must_use]
    pub fn source_text(&self) -> Option<&str> {
        match self {
            Self::Annotation(text)
            | Self::Directive(text)
            | Self::Identifier(text)
            | Self::StringLiteral(text)
            | Self::Other(text)
            | Self::Literal(LiteralValue::Number(text)) => Some(text.as_str()),
            Self::Keyword(keyword) => Some(keyword.text()),
            Self::TypeMark(primitive) => Some(primitive.text()),
            Self::Operator(operator) => Some(operator.text()),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_declaration_modifier(&self) -> bool {
        matches!(
            self,
            Self::Identifier(text)
                if matches!(
                    text.as_str(),
                    "lowp"
                        | "mediump"
                        | "highp"
                        | "flat"
                        | "smooth"
                        | "noperspective"
                        | "centroid"
                        | "sample"
                        | "invariant"
                )
        ) || matches!(self, Self::Keyword(KeywordType::Const))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorType {
    Arithmetic(ArithmeticOperator),
    Logical(LogicalOperator),
    Relational(RelationalOperator),
    Equality(EqualityOperator),
    Bitwise(BitwiseOperator),
    Assignment(AssignmentOperator),
    Increment(IncrementOperator),
    Access(AccessOperator),
    Conditional(ConditionalOperator),
    Grouping(GroupingOperator),
    Subscript(SubscriptOperator),
    Comma(CommaOperator),
}

impl OperatorType {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "+" => Some(Self::Arithmetic(ArithmeticOperator::Add)),
            "-" => Some(Self::Arithmetic(ArithmeticOperator::Subtract)),
            "*" => Some(Self::Arithmetic(ArithmeticOperator::Multiply)),
            "/" => Some(Self::Arithmetic(ArithmeticOperator::Divide)),
            "%" => Some(Self::Arithmetic(ArithmeticOperator::Remainder)),
            "!" => Some(Self::Logical(LogicalOperator::Not)),
            "&&" => Some(Self::Logical(LogicalOperator::And)),
            "||" => Some(Self::Logical(LogicalOperator::Or)),
            "^^" => Some(Self::Logical(LogicalOperator::Xor)),
            "<" => Some(Self::Relational(RelationalOperator::Less)),
            ">" => Some(Self::Relational(RelationalOperator::Greater)),
            "<=" => Some(Self::Relational(RelationalOperator::LessEqual)),
            ">=" => Some(Self::Relational(RelationalOperator::GreaterEqual)),
            "==" => Some(Self::Equality(EqualityOperator::Equal)),
            "!=" => Some(Self::Equality(EqualityOperator::NotEqual)),
            "~" => Some(Self::Bitwise(BitwiseOperator::Not)),
            "&" => Some(Self::Bitwise(BitwiseOperator::And)),
            "^" => Some(Self::Bitwise(BitwiseOperator::Xor)),
            "|" => Some(Self::Bitwise(BitwiseOperator::Or)),
            "<<" => Some(Self::Bitwise(BitwiseOperator::ShiftLeft)),
            ">>" => Some(Self::Bitwise(BitwiseOperator::ShiftRight)),
            "=" => Some(Self::Assignment(AssignmentOperator::Assign)),
            "+=" => Some(Self::Assignment(AssignmentOperator::AddAssign)),
            "-=" => Some(Self::Assignment(AssignmentOperator::SubtractAssign)),
            "*=" => Some(Self::Assignment(AssignmentOperator::MultiplyAssign)),
            "/=" => Some(Self::Assignment(AssignmentOperator::DivideAssign)),
            "%=" => Some(Self::Assignment(AssignmentOperator::RemainderAssign)),
            "<<=" => Some(Self::Assignment(AssignmentOperator::ShiftLeftAssign)),
            ">>=" => Some(Self::Assignment(AssignmentOperator::ShiftRightAssign)),
            "&=" => Some(Self::Assignment(AssignmentOperator::AndAssign)),
            "^=" => Some(Self::Assignment(AssignmentOperator::XorAssign)),
            "|=" => Some(Self::Assignment(AssignmentOperator::OrAssign)),
            "++" => Some(Self::Increment(IncrementOperator::Increment)),
            "--" => Some(Self::Increment(IncrementOperator::Decrement)),
            "." => Some(Self::Access(AccessOperator::Member)),
            "?" => Some(Self::Conditional(ConditionalOperator::Question)),
            ":" => Some(Self::Conditional(ConditionalOperator::Colon)),
            "(" => Some(Self::Grouping(GroupingOperator::LeftParen)),
            ")" => Some(Self::Grouping(GroupingOperator::RightParen)),
            "[" => Some(Self::Subscript(SubscriptOperator::LeftSquare)),
            "]" => Some(Self::Subscript(SubscriptOperator::RightSquare)),
            "," => Some(Self::Comma(CommaOperator::Comma)),
            _ => None,
        }
    }

    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Arithmetic(operator) => operator.text(),
            Self::Logical(operator) => operator.text(),
            Self::Relational(operator) => operator.text(),
            Self::Equality(operator) => operator.text(),
            Self::Bitwise(operator) => operator.text(),
            Self::Assignment(operator) => operator.text(),
            Self::Increment(operator) => operator.text(),
            Self::Access(operator) => operator.text(),
            Self::Conditional(operator) => operator.text(),
            Self::Grouping(operator) => operator.text(),
            Self::Subscript(operator) => operator.text(),
            Self::Comma(operator) => operator.text(),
        }
    }

    #[must_use]
    pub const fn category(self) -> OperatorCategory {
        match self {
            Self::Arithmetic(_) => OperatorCategory::Arithmetic,
            Self::Logical(_) => OperatorCategory::Logical,
            Self::Relational(_) => OperatorCategory::Relational,
            Self::Equality(_) => OperatorCategory::Equality,
            Self::Bitwise(_) => OperatorCategory::Bitwise,
            Self::Assignment(_) => OperatorCategory::Assignment,
            Self::Increment(_) => OperatorCategory::Increment,
            Self::Access(_) => OperatorCategory::Access,
            Self::Conditional(_) => OperatorCategory::Conditional,
            Self::Grouping(_) => OperatorCategory::Grouping,
            Self::Subscript(_) => OperatorCategory::Subscript,
            Self::Comma(_) => OperatorCategory::Comma,
        }
    }

    #[must_use]
    pub const fn is_assignment(self) -> bool {
        matches!(self, Self::Assignment(_))
    }

    #[must_use]
    pub const fn is_simple_assignment(self) -> bool {
        matches!(self, Self::Assignment(AssignmentOperator::Assign))
    }

    #[must_use]
    pub const fn is_compound_assignment(self) -> bool {
        matches!(
            self,
            Self::Assignment(
                AssignmentOperator::AddAssign
                    | AssignmentOperator::SubtractAssign
                    | AssignmentOperator::MultiplyAssign
                    | AssignmentOperator::DivideAssign
                    | AssignmentOperator::RemainderAssign
                    | AssignmentOperator::ShiftLeftAssign
                    | AssignmentOperator::ShiftRightAssign
                    | AssignmentOperator::AndAssign
                    | AssignmentOperator::XorAssign
                    | AssignmentOperator::OrAssign
            )
        )
    }

    #[must_use]
    pub const fn is_comparison(self) -> bool {
        matches!(self, Self::Relational(_) | Self::Equality(_))
    }

    #[must_use]
    pub const fn is_scalar_binary(self) -> bool {
        matches!(self, Self::Arithmetic(_))
    }

    #[must_use]
    pub const fn is_additive(self) -> bool {
        matches!(
            self,
            Self::Arithmetic(ArithmeticOperator::Add | ArithmeticOperator::Subtract)
        )
    }

    #[must_use]
    pub const fn is_multiplicative(self) -> bool {
        matches!(
            self,
            Self::Arithmetic(
                ArithmeticOperator::Multiply
                    | ArithmeticOperator::Divide
                    | ArithmeticOperator::Remainder
            )
        )
    }

    #[must_use]
    pub const fn is_logical_not(self) -> bool {
        matches!(self, Self::Logical(LogicalOperator::Not))
    }

    #[must_use]
    pub const fn is_member_access(self) -> bool {
        matches!(self, Self::Access(AccessOperator::Member))
    }

    #[must_use]
    pub const fn is_unary_boundary(self) -> bool {
        matches!(
            self,
            Self::Assignment(_)
                | Self::Conditional(_)
                | Self::Relational(_)
                | Self::Equality(_)
                | Self::Logical(LogicalOperator::Not)
                | Self::Arithmetic(_)
                | Self::Bitwise(BitwiseOperator::Not)
                | Self::Comma(_)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorCategory {
    Arithmetic,
    Logical,
    Relational,
    Equality,
    Bitwise,
    Assignment,
    Increment,
    Access,
    Conditional,
    Grouping,
    Subscript,
    Comma,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl ArithmeticOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalOperator {
    Not,
    And,
    Or,
    Xor,
}

impl LogicalOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Not => "!",
            Self::And => "&&",
            Self::Or => "||",
            Self::Xor => "^^",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationalOperator {
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
}

impl RelationalOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::Greater => ">",
            Self::LessEqual => "<=",
            Self::GreaterEqual => ">=",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EqualityOperator {
    Equal,
    NotEqual,
}

impl EqualityOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BitwiseOperator {
    Not,
    And,
    Xor,
    Or,
    ShiftLeft,
    ShiftRight,
}

impl BitwiseOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Not => "~",
            Self::And => "&",
            Self::Xor => "^",
            Self::Or => "|",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentOperator {
    Assign,
    AddAssign,
    SubtractAssign,
    MultiplyAssign,
    DivideAssign,
    RemainderAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    AndAssign,
    XorAssign,
    OrAssign,
}

impl AssignmentOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Assign => "=",
            Self::AddAssign => "+=",
            Self::SubtractAssign => "-=",
            Self::MultiplyAssign => "*=",
            Self::DivideAssign => "/=",
            Self::RemainderAssign => "%=",
            Self::ShiftLeftAssign => "<<=",
            Self::ShiftRightAssign => ">>=",
            Self::AndAssign => "&=",
            Self::XorAssign => "^=",
            Self::OrAssign => "|=",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IncrementOperator {
    Increment,
    Decrement,
}

impl IncrementOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Increment => "++",
            Self::Decrement => "--",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessOperator {
    Member,
}

impl AccessOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Member => ".",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionalOperator {
    Question,
    Colon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupingOperator {
    LeftParen,
    RightParen,
}

impl GroupingOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::LeftParen => "(",
            Self::RightParen => ")",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptOperator {
    LeftSquare,
    RightSquare,
}

impl SubscriptOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::LeftSquare => "[",
            Self::RightSquare => "]",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommaOperator {
    Comma,
}

impl CommaOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Comma => ",",
        }
    }
}

impl ConditionalOperator {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Question => "?",
            Self::Colon => ":",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeywordType {
    Attribute,
    Const,
    Do,
    Else,
    For,
    If,
    In,
    Layout,
    Out,
    Precision,
    Return,

    Struct,
    Switch,
    Uniform,
    Varying,
    Void,
    While,
}

impl KeywordType {
    #[must_use]
    pub const fn parse(text: &str) -> Option<Self> {
        match text.as_bytes() {
            b"attribute" => Some(Self::Attribute),
            b"const" => Some(Self::Const),
            b"do" => Some(Self::Do),
            b"else" => Some(Self::Else),
            b"for" => Some(Self::For),
            b"if" => Some(Self::If),
            b"in" => Some(Self::In),
            b"layout" => Some(Self::Layout),
            b"out" => Some(Self::Out),
            b"precision" => Some(Self::Precision),
            b"return" => Some(Self::Return),
            b"struct" => Some(Self::Struct),
            b"switch" => Some(Self::Switch),
            b"uniform" => Some(Self::Uniform),
            b"varying" => Some(Self::Varying),
            b"void" => Some(Self::Void),
            b"while" => Some(Self::While),
            _ => None,
        }
    }

    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Attribute => "attribute",
            Self::Const => "const",
            Self::Do => "do",
            Self::Else => "else",
            Self::For => "for",
            Self::If => "if",
            Self::In => "in",
            Self::Layout => "layout",
            Self::Out => "out",
            Self::Precision => "precision",
            Self::Return => "return",
            Self::Struct => "struct",
            Self::Switch => "switch",
            Self::Uniform => "uniform",
            Self::Varying => "varying",
            Self::Void => "void",
            Self::While => "while",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveType {
    Bool,
    Int,
    Uint,
    Float,
    Double,
    Vec(u8),
    IVec(u8),
    UVec(u8),
    BVec(u8),
    Mat { columns: u8, rows: u8 },
    Sampler2D,
    Sampler2DArray,
    SamplerCube,
}

impl PrimitiveType {
    #[must_use]
    pub const fn parse(text: &str) -> Option<Self> {
        match text.as_bytes() {
            b"bool" => Some(Self::Bool),
            b"int" => Some(Self::Int),
            b"uint" => Some(Self::Uint),
            b"float" => Some(Self::Float),
            b"double" => Some(Self::Double),
            b"vec2" => Some(Self::Vec(2)),
            b"vec3" => Some(Self::Vec(3)),
            b"vec4" => Some(Self::Vec(4)),
            b"ivec2" => Some(Self::IVec(2)),
            b"ivec3" => Some(Self::IVec(3)),
            b"ivec4" => Some(Self::IVec(4)),
            b"uvec2" => Some(Self::UVec(2)),
            b"uvec3" => Some(Self::UVec(3)),
            b"uvec4" => Some(Self::UVec(4)),
            b"bvec2" => Some(Self::BVec(2)),
            b"bvec3" => Some(Self::BVec(3)),
            b"bvec4" => Some(Self::BVec(4)),
            b"mat2" | b"mat2x2" => Some(Self::Mat {
                columns: 2,
                rows: 2,
            }),
            b"mat2x3" => Some(Self::Mat {
                columns: 2,
                rows: 3,
            }),
            b"mat2x4" => Some(Self::Mat {
                columns: 2,
                rows: 4,
            }),
            b"mat3x2" => Some(Self::Mat {
                columns: 3,
                rows: 2,
            }),
            b"mat3" | b"mat3x3" => Some(Self::Mat {
                columns: 3,
                rows: 3,
            }),
            b"mat3x4" => Some(Self::Mat {
                columns: 3,
                rows: 4,
            }),
            b"mat4x2" => Some(Self::Mat {
                columns: 4,
                rows: 2,
            }),
            b"mat4x3" => Some(Self::Mat {
                columns: 4,
                rows: 3,
            }),
            b"mat4" | b"mat4x4" => Some(Self::Mat {
                columns: 4,
                rows: 4,
            }),
            b"sampler2D" => Some(Self::Sampler2D),
            b"sampler2DArray" => Some(Self::Sampler2DArray),
            b"samplerCube" => Some(Self::SamplerCube),
            _ => None,
        }
    }

    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Uint => "uint",
            Self::Float => "float",
            Self::Double => "double",
            Self::Vec(2) => "vec2",
            Self::Vec(3) => "vec3",
            Self::Vec(4) => "vec4",
            Self::IVec(2) => "ivec2",
            Self::IVec(3) => "ivec3",
            Self::IVec(4) => "ivec4",
            Self::UVec(2) => "uvec2",
            Self::UVec(3) => "uvec3",
            Self::UVec(4) => "uvec4",
            Self::BVec(2) => "bvec2",
            Self::BVec(3) => "bvec3",
            Self::BVec(4) => "bvec4",
            Self::Mat {
                columns: 2,
                rows: 2,
            } => "mat2",
            Self::Mat {
                columns: 2,
                rows: 3,
            } => "mat2x3",
            Self::Mat {
                columns: 2,
                rows: 4,
            } => "mat2x4",
            Self::Mat {
                columns: 3,
                rows: 2,
            } => "mat3x2",
            Self::Mat {
                columns: 3,
                rows: 3,
            } => "mat3",
            Self::Mat {
                columns: 3,
                rows: 4,
            } => "mat3x4",
            Self::Mat {
                columns: 4,
                rows: 2,
            } => "mat4x2",
            Self::Mat {
                columns: 4,
                rows: 3,
            } => "mat4x3",
            Self::Mat {
                columns: 4,
                rows: 4,
            } => "mat4",
            Self::Sampler2D => "sampler2D",
            Self::Sampler2DArray => "sampler2DArray",
            Self::SamplerCube => "samplerCube",
            Self::Vec(_) | Self::IVec(_) | Self::UVec(_) | Self::BVec(_) | Self::Mat { .. } => "",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiteralValue {
    Number(SmolStr),
}
