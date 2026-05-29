//! Function parameter declaration scanning.

use super::{
    super::TokenSearch,
    parameters::{FunctionParameterQualifier, FunctionParameterTypeMode, ParameterSegmentEnd},
    types::DeclarationCandidate,
};
use crate::lexer::{Token, TokenKind};

/// Function prototype or header parameter-list range containing a candidate.
#[derive(Clone, Copy)]
pub(super) struct FunctionParameterList {
    /// Opening parenthesis of the parameter list.
    open: usize,
    /// Closing parenthesis of the parameter list.
    close: usize,
}

impl TryFrom<DeclarationCandidate<'_, '_>> for FunctionParameterList {
    type Error = ();

    fn try_from(candidate: DeclarationCandidate<'_, '_>) -> Result<Self, Self::Error> {
        let parentheses = EnclosingParentheses::try_from(candidate)?;
        let open = parentheses.open;
        let close = parentheses.close().ok_or(())?;
        let list = Self { open, close };
        list.has_function_header_shape(candidate.tokens)
            .then_some(list)
            .ok_or(())
    }
}

impl FunctionParameterList {
    /// Returns whether these parentheses form a function prototype/header.
    fn has_function_header_shape(self, tokens: &[Token<'_>]) -> bool {
        let search = TokenSearch::new(tokens);
        let Some(function_name) = search.previous_non_comment(self.open) else {
            return false;
        };
        let TokenKind::Identifier(name) = tokens[function_name].kind else {
            return false;
        };
        if FunctionHeaderName::from(name).is_control_keyword() {
            return false;
        }
        let Some(return_type) = search.previous_non_comment(function_name) else {
            return false;
        };
        if !matches!(tokens[return_type].kind, TokenKind::Identifier(_)) {
            return false;
        }
        let Some(after_close) = search.next_non_comment(self.close + 1) else {
            return false;
        };
        matches!(
            tokens[after_close].kind,
            TokenKind::Semicolon | TokenKind::LeftBrace
        )
    }
}

/// Parentheses enclosing a candidate declaration token.
#[derive(Clone, Copy)]
struct EnclosingParentheses<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Opening parenthesis index.
    open: usize,
}

impl<'tokens, 'src> TryFrom<DeclarationCandidate<'tokens, 'src>>
    for EnclosingParentheses<'tokens, 'src>
{
    type Error = ();

    /// Finds the innermost opening parenthesis enclosing `candidate`.
    fn try_from(candidate: DeclarationCandidate<'tokens, 'src>) -> Result<Self, Self::Error> {
        let mut depth = 0usize;
        for index in (0..candidate.start).rev() {
            match candidate.tokens[index].kind {
                TokenKind::RightParen => depth += 1,
                TokenKind::LeftParen if depth == 0 => {
                    return Ok(Self {
                        tokens: candidate.tokens,
                        open: index,
                    });
                }
                TokenKind::LeftParen => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        Err(())
    }
}

impl EnclosingParentheses<'_, '_> {
    /// Returns the matching closing parenthesis.
    fn close(self) -> Option<usize> {
        let mut depth = 0usize;
        for index in self.open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// Function-header name classifier.
#[derive(Clone, Copy)]
struct FunctionHeaderName<'src> {
    /// Source spelling.
    name: &'src str,
}

impl<'src> From<&'src str> for FunctionHeaderName<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}

impl FunctionHeaderName<'_> {
    /// Returns whether this identifier starts a control-flow header.
    fn is_control_keyword(self) -> bool {
        matches!(self.name, "for" | "if" | "switch" | "while")
    }
}

/// One function parameter that is visible only inside its function body.
#[derive(Clone, Copy)]
pub struct FunctionParameterDeclaration<'src> {
    /// Parameter name.
    name: &'src str,
    /// Parameter type spelling.
    ty: &'src str,
    /// First token where the parameter is visible.
    visible_start: usize,
    /// First token outside the function body.
    scope_end: usize,
}

impl<'src> FunctionParameterDeclaration<'src> {
    /// Returns the parameter name.
    pub(crate) const fn name(self) -> &'src str {
        self.name
    }

    /// Returns the parameter type spelling.
    pub(crate) const fn ty(self) -> &'src str {
        self.ty
    }

    /// Returns the first token where this parameter is visible.
    pub(crate) const fn visible_start(self) -> usize {
        self.visible_start
    }

    /// Returns first token outside this parameter's body scope.
    pub(crate) const fn scope_end(self) -> usize {
        self.scope_end
    }
}

/// Function-body-scoped parameter declarations.
pub struct FunctionParameterDeclarations<'src> {
    /// Collected parameters.
    items: Vec<FunctionParameterDeclaration<'src>>,
    /// Next item index.
    index: usize,
}

impl<'src> From<&[Token<'src>]> for FunctionParameterDeclarations<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        Self::from_tokens(tokens, FunctionParameterTypeMode::Builtins)
    }
}

impl<'src> FunctionParameterDeclarations<'src> {
    /// Collects parameters accepted by `type_mode`.
    pub(crate) fn from_tokens(
        tokens: &[Token<'src>],
        type_mode: FunctionParameterTypeMode,
    ) -> Self {
        Self {
            items: FunctionParameterScanner { tokens, type_mode }.collect(),
            index: 0,
        }
    }
}

impl<'src> Iterator for FunctionParameterDeclarations<'src> {
    type Item = FunctionParameterDeclaration<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.items.get(self.index).copied()?;
        self.index += 1;
        Some(item)
    }
}

/// Scanner for function definitions with parameter facts.
struct FunctionParameterScanner<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Accepted parameter type names.
    type_mode: FunctionParameterTypeMode,
}

impl<'src> FunctionParameterScanner<'_, 'src> {
    /// Collects body-scoped parameters from function definitions.
    fn collect(self) -> Vec<FunctionParameterDeclaration<'src>> {
        let mut items = Vec::new();
        let search = TokenSearch::new(self.tokens);
        for function_name in 0..self.tokens.len() {
            let TokenKind::Identifier(name) = self.tokens[function_name].kind else {
                continue;
            };
            if FunctionHeaderName::from(name).is_control_keyword() {
                continue;
            }
            let Some(open) = search.next_non_comment(function_name + 1) else {
                continue;
            };
            if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
                continue;
            }
            let Some(return_type) = search.previous_non_comment(function_name) else {
                continue;
            };
            if !matches!(self.tokens[return_type].kind, TokenKind::Identifier(_)) {
                continue;
            }
            let Some(close) = EnclosingFunctionHeader {
                tokens: self.tokens,
                open,
            }
            .close() else {
                continue;
            };
            let Some(body_open) = search.next_non_comment(close + 1) else {
                continue;
            };
            if !matches!(self.tokens[body_open].kind, TokenKind::LeftBrace) {
                continue;
            }
            let body_close = EnclosingFunctionHeader {
                tokens: self.tokens,
                open,
            }
            .body_close(body_open);
            items.extend(FunctionParameterListItems {
                tokens: self.tokens,
                start: open + 1,
                end: close,
                visible_start: body_open + 1,
                scope_end: body_close,
                type_mode: self.type_mode,
            });
        }
        items
    }
}

/// Function header delimiter helper.
#[derive(Clone, Copy)]
struct EnclosingFunctionHeader<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Opening parameter parenthesis.
    open: usize,
}

impl EnclosingFunctionHeader<'_, '_> {
    /// Returns the matching right parenthesis.
    fn close(self) -> Option<usize> {
        let mut depth = 0usize;
        for index in self.open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the matching right brace for the body.
    fn body_close(self, body_open: usize) -> usize {
        let mut depth = 0usize;
        for index in body_open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftBrace => depth += 1,
                TokenKind::RightBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index;
                    }
                }
                _ => {}
            }
        }
        self.tokens.len()
    }
}

/// Iterator over parameter declarations inside one function definition header.
struct FunctionParameterListItems<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Current parameter segment start.
    start: usize,
    /// Exclusive parameter-list end.
    end: usize,
    /// First token where parameters are visible.
    visible_start: usize,
    /// First token outside the function body.
    scope_end: usize,
    /// Accepted parameter type names.
    type_mode: FunctionParameterTypeMode,
}

impl<'src> Iterator for FunctionParameterListItems<'_, 'src> {
    type Item = FunctionParameterDeclaration<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.start < self.end {
            let segment_start = self.start;
            let segment_end = ParameterSegmentEnd {
                tokens: self.tokens,
                start: segment_start,
                end: self.end,
            }
            .end();
            self.start = segment_end.saturating_add(1);
            let mut ty = None;
            for token in self.tokens.iter().take(segment_end).skip(segment_start) {
                let TokenKind::Identifier(text) = token.kind else {
                    continue;
                };
                if FunctionParameterQualifier::from(text).is_qualifier() {
                    continue;
                }
                if ty.is_none() && self.type_mode.accepts(text) {
                    ty = Some(text);
                    continue;
                }
                if let Some(ty) = ty {
                    return Some(FunctionParameterDeclaration {
                        name: text,
                        ty,
                        visible_start: self.visible_start,
                        scope_end: self.scope_end,
                    });
                }
            }
        }
        None
    }
}
