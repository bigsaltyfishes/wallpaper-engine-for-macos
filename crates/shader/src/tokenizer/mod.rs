//! Lifetime-free tokenization for Wallpaper Engine shader sources.

mod cursor;
mod expression;
mod facts;
mod range;
mod stream;
mod token;

pub use cursor::{IdentifierToken, TokenCursor, TokenMatcher};
pub use expression::{CallFact, ExpressionFact, ExpressionShape, FloatConstructor, OperatorFact};
pub use facts::{
    ConditionFact, DeclarationFact, FunctionParameterFact, FunctionSignatureFact, StatementFact,
    StatementKind, TypedTokenFacts,
};
pub use range::TokenIndexRange;
pub use stream::TokenStream;
pub use token::{
    AccessOperator, ArithmeticOperator, AssignmentOperator, BitwiseOperator, CommaOperator,
    ConditionalOperator, EqualityOperator, GroupingOperator, IncrementOperator, KeywordType,
    LiteralValue, LogicalOperator, OperatorCategory, OperatorType, PrimitiveType,
    RelationalOperator, SubscriptOperator, Token, TypedToken,
};
