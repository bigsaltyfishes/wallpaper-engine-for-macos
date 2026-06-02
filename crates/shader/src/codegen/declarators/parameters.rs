//! Function parameter declarator helpers.

use crate::tokenizer::{KeywordType, TypedToken};

/// Function parameter qualifier predicates.
pub(crate) struct FunctionParameterQualifier;

impl FunctionParameterQualifier {
    /// Returns whether this typed token is a function parameter qualifier.
    #[must_use]
    pub(crate) fn is_token(kind: &TypedToken) -> bool {
        match kind {
            TypedToken::Keyword(keyword) => matches!(
                keyword,
                KeywordType::Const | KeywordType::In | KeywordType::Out
            ),
            TypedToken::Identifier(name) => {
                matches!(name.as_str(), "inout" | "lowp" | "mediump" | "highp")
            }
            _ => false,
        }
    }
}
