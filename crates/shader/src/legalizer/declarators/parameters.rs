//! Function parameter qualifiers and segment scanning.

use super::types::LocalTypeName;
use crate::lexer::{Token, TokenKind};

/// Parameter type-name matcher used by callers with different fact needs.
#[derive(Clone, Copy)]
pub enum FunctionParameterTypeMode {
    /// Built-in scalar/vector/matrix types only.
    Builtins,
    /// Any identifier that syntactically appears in parameter type position.
    Any,
}

impl FunctionParameterTypeMode {
    /// Returns whether `name` should be treated as the parameter type token.
    pub(super) fn accepts(self, name: &str) -> bool {
        match self {
            Self::Builtins => LocalTypeName::from(name).is_local(),
            Self::Any => true,
        }
    }
}

/// Finds the end of one top-level comma-delimited parameter segment.
pub(super) struct ParameterSegmentEnd<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Segment start.
    pub(super) start: usize,
    /// Parameter-list end.
    pub(super) end: usize,
}

impl ParameterSegmentEnd<'_, '_> {
    /// Returns the exclusive segment end.
    pub(super) fn end(self) -> usize {
        let mut depth = 0usize;
        for index in self.start..self.end {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => return index,
                _ => {}
            }
        }
        self.end
    }
}

/// Function parameter storage qualifier.
#[derive(Clone, Copy)]
pub struct FunctionParameterQualifier<'src> {
    /// Source spelling.
    name: &'src str,
}

impl<'src> From<&'src str> for FunctionParameterQualifier<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}

impl FunctionParameterQualifier<'_> {
    /// Returns whether this token is a parameter qualifier.
    pub(crate) fn is_qualifier(self) -> bool {
        matches!(
            self.name,
            "const" | "in" | "out" | "inout" | "lowp" | "mediump" | "highp"
        )
    }
}
