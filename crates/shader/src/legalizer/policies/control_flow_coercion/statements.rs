use super::expr::Lvalue;
use crate::{
    SourceSpan,
    legalizer::{DeclarationDeclarators, LocalDeclaration, LocalDeclarationStart, TokenSearch},
    lexer::{Token, TokenKind},
};

/// Token-backed statement stream.
pub(super) struct StatementStream<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Next token index to inspect.
    cursor: usize,
}

impl<'tokens, 'src> From<&'tokens [Token<'src>]> for StatementStream<'tokens, 'src> {
    fn from(tokens: &'tokens [Token<'src>]) -> Self {
        Self { tokens, cursor: 0 }
    }
}

impl<'tokens, 'src> Iterator for StatementStream<'tokens, 'src> {
    type Item = Statement<'tokens, 'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let search = TokenSearch::new(self.tokens);
        let start = search.next_non_comment(self.cursor)?;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    self.cursor = index + 1;
                    return Some(Statement {
                        tokens: self.tokens,
                        start,
                        semicolon: index,
                    });
                }
                TokenKind::LeftBrace | TokenKind::RightBrace
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    self.cursor = index + 1;
                    return self.next();
                }
                _ => {}
            }
        }

        self.cursor = self.tokens.len();
        None
    }
}

/// One token-backed statement ending at a semicolon.
#[derive(Clone, Copy)]
pub(super) struct Statement<'tokens, 'src> {
    /// Full token stream.
    pub(in crate::legalizer::policies::control_flow_coercion) tokens: &'tokens [Token<'src>],
    /// First non-comment token in the statement.
    pub(in crate::legalizer::policies::control_flow_coercion) start: usize,
    /// Statement semicolon token.
    pub(in crate::legalizer::policies::control_flow_coercion) semicolon: usize,
}

impl<'tokens, 'src> Statement<'tokens, 'src> {
    /// Returns the expression span between `=` and this statement's semicolon.
    pub(super) fn rhs_span(self, equals: usize) -> Option<SourceSpan> {
        let search = TokenSearch::new(self.tokens);
        let start = search.next_non_comment(equals + 1)?;
        let end = search.previous_non_comment(self.semicolon)?;
        SourceSpan::new(self.tokens[start].span.start(), self.tokens[end].span.end()).ok()
    }

    /// Returns the `=` token for a simple lvalue assignment statement.
    pub(super) fn lvalue_assignment(self) -> Option<(Lvalue<'src>, usize)> {
        let tokens = self.tokens;
        let search = TokenSearch::new(tokens);
        let equals = (self.start..self.semicolon).find(|index| {
            if !matches!(tokens[*index].kind, TokenKind::Punctuation('=')) {
                return false;
            }
            let previous = search.previous_non_comment(*index);
            let next = search.next_non_comment(*index + 1);
            !matches!(
                previous.map(|previous| tokens[previous].kind),
                Some(TokenKind::Punctuation(
                    '=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | '%'
                ))
            ) && !matches!(
                next.map(|next| tokens[next].kind),
                Some(TokenKind::Punctuation('='))
            )
        })?;
        let lhs_end = search.previous_non_comment(equals)?;
        let lhs = Lvalue::ending_at(tokens, lhs_end)?;
        (lhs.start == self.start).then_some((lhs, equals))
    }

    /// Returns declarators when this statement starts with a local declaration
    /// of `ty`.
    pub(super) fn declaration_declarators(
        self,
        ty: &str,
    ) -> Option<DeclarationDeclarators<'tokens, 'src>> {
        let declaration = self.local_declaration()?;
        (declaration.ty() == ty).then(|| DeclarationDeclarators::new(self.tokens, declaration))
    }

    /// Returns declarators when this statement starts with a local declaration.
    pub(super) fn local_declaration_declarators(
        self,
    ) -> Option<DeclarationDeclarators<'tokens, 'src>> {
        let declaration = self.local_declaration()?;
        Some(DeclarationDeclarators::new(self.tokens, declaration))
    }

    /// Returns the local declaration at this statement start.
    fn local_declaration(self) -> Option<LocalDeclaration<'src>> {
        LocalDeclaration::try_from(LocalDeclarationStart {
            tokens: self.tokens,
            start: self.start,
        })
        .ok()
    }
}
