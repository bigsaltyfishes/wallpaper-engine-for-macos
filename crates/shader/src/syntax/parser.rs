//! Cursor-based syntax parser implementation.

use super::{
    FunctionDecl, ParsingContext, PreprocessorDirective, ShaderAnnotation, ShaderDeclaration,
    ShaderModule, SyntaxItem, TopLevelQualifier,
    declaration::{DeclarationArraySuffix, DeclarationKind, DeclarationLayout},
};
use crate::{
    ShaderDiagnostic, ShaderResult, SourceSpan,
    lexer::{Token, TokenKind},
};

/// Cursor-based parser over the borrowed token stream for one source.
pub(super) struct Parser<'context, 'src> {
    /// Owning parse context that provides stage, source, and token storage.
    pub(super) context: &'context ParsingContext<'src>,
    /// Borrowed tokens being parsed in source order.
    pub(super) tokens: &'context [Token<'src>],
    /// Current token offset within `tokens`.
    pub(super) cursor: usize,
}

impl<'src> Parser<'_, 'src> {
    /// Parses the full token stream into a module of top-level syntax items.
    pub(super) fn parse_module(&mut self) -> ShaderResult<ShaderModule<'src>> {
        let mut items = Vec::with_capacity(self.tokens.len().min(64));

        while self.cursor < self.tokens.len() {
            if let Some(item) = self.parse_next_item()? {
                items.push(item);
            }
        }

        Ok(ShaderModule::new(
            self.context.stage(),
            self.context.source(),
            self.context.tokens().to_vec(),
            items,
        ))
    }

    /// Parses the next top-level token sequence into a syntax item.
    fn parse_next_item(&mut self) -> ShaderResult<Option<SyntaxItem<'src>>> {
        let token = self.tokens[self.cursor];
        match token.kind {
            TokenKind::Annotation(text) => {
                self.cursor += 1;
                Ok(Some(SyntaxItem::Annotation(
                    ShaderAnnotation::from_token_text(text, token.span),
                )))
            }
            TokenKind::Directive(text) => {
                self.cursor += 1;
                Ok(Some(SyntaxItem::Directive(
                    PreprocessorDirective::from_token_text(text, token.span),
                )))
            }
            TokenKind::Comment(_) => {
                self.cursor += 1;
                Ok(None)
            }
            TokenKind::Identifier("struct") => self.parse_struct_declaration(),
            TokenKind::Identifier(_) => self.parse_identifier_item(),
            _ => {
                self.cursor += 1;
                Ok(Some(SyntaxItem::Opaque(token.span)))
            }
        }
    }

    /// Parses an identifier-led top-level item as a function or declaration.
    fn parse_identifier_item(&mut self) -> ShaderResult<Option<SyntaxItem<'src>>> {
        if let Some(function) = self.try_parse_function()? {
            return Ok(Some(SyntaxItem::Function(function)));
        }

        Ok(Some(SyntaxItem::Declaration(
            self.parse_semicolon_declaration()?,
        )))
    }

    /// Parses a function signature and balanced body starting at the cursor.
    fn try_parse_function(&mut self) -> ShaderResult<Option<FunctionDecl<'src>>> {
        let start = self.cursor;
        let Some(open_paren) = self.find_top_level_left_paren_before_terminator(start) else {
            return Ok(None);
        };

        let Some(name_index) = self.previous_non_comment(open_paren) else {
            return Ok(None);
        };
        let Some(return_type_index) = self.previous_non_comment(name_index) else {
            return Ok(None);
        };

        let name_token = self.tokens[name_index];
        let return_type_token = self.tokens[return_type_index];
        let (TokenKind::Identifier(name), TokenKind::Identifier(return_type)) =
            (name_token.kind, return_type_token.kind)
        else {
            return Ok(None);
        };

        let close_paren = self.find_matching_paren(open_paren)?;
        let Some(body_open) = self.next_non_comment(close_paren + 1) else {
            return Ok(None);
        };
        if !matches!(self.tokens[body_open].kind, TokenKind::LeftBrace) {
            return Ok(None);
        }

        let body_close = self.find_matching_brace(body_open)?;
        let signature = SourceSpan::new(
            self.tokens[start].span.start(),
            self.tokens[close_paren].span.end(),
        )?;
        let parameters = SourceSpan::new(
            self.tokens[open_paren].span.end(),
            self.tokens[close_paren].span.start(),
        )?;
        let body = SourceSpan::new(
            self.tokens[body_open].span.start(),
            self.tokens[body_close].span.end(),
        )?;
        let span = SourceSpan::new(
            self.tokens[start].span.start(),
            self.tokens[body_close].span.end(),
        )?;

        self.cursor = body_close + 1;

        Ok(Some(FunctionDecl::new(
            return_type,
            name,
            parameters,
            signature,
            body,
            span,
        )))
    }

    /// Parses tokens through the next semicolon as a top-level declaration.
    fn parse_semicolon_declaration(&mut self) -> ShaderResult<ShaderDeclaration<'src>> {
        let start = self.cursor;
        let mut end = start;

        while end < self.tokens.len() {
            if matches!(self.tokens[end].kind, TokenKind::Semicolon) {
                let declaration = self.declaration_from_range(start, end)?;
                self.cursor = end + 1;
                return Ok(declaration);
            }

            if matches!(
                self.tokens[end].kind,
                TokenKind::LeftBrace | TokenKind::RightBrace
            ) {
                break;
            }

            end += 1;
        }

        self.cursor += 1;
        Ok(ShaderDeclaration::new(
            DeclarationKind::Other,
            None,
            None,
            self.tokens[start].kind.identifier_text(),
            None,
            None,
            self.tokens[start].span,
        ))
    }

    /// Builds declaration metadata from a semicolon-terminated token range.
    fn declaration_from_range(
        &self,
        start: usize,
        semicolon: usize,
    ) -> ShaderResult<ShaderDeclaration<'src>> {
        let head = self.declaration_head(start, semicolon);
        let qualifier = head.qualifier;
        let kind = if qualifier.is_some() {
            DeclarationKind::Interface
        } else {
            DeclarationKind::Other
        };

        Ok(ShaderDeclaration::new(
            kind,
            qualifier,
            head.type_name,
            head.name,
            head.array_suffix,
            head.layout,
            SourceSpan::new(
                self.tokens[start].span.start(),
                self.tokens[semicolon].span.end(),
            )?,
        ))
    }

    /// Parses a struct declaration and its balanced body.
    fn parse_struct_declaration(&mut self) -> ShaderResult<Option<SyntaxItem<'src>>> {
        let start = self.cursor;
        let name = self
            .tokens
            .get(start + 1)
            .and_then(|token| token.kind.identifier_text());
        let Some(open_brace) = self.find_token(start, TokenKindMatcher::LeftBrace) else {
            return Ok(Some(SyntaxItem::Declaration(
                self.parse_semicolon_declaration()?,
            )));
        };

        let close_brace = self.find_matching_brace(open_brace)?;
        let semicolon = if self
            .tokens
            .get(close_brace + 1)
            .is_some_and(|token| matches!(token.kind, TokenKind::Semicolon))
        {
            close_brace + 1
        } else {
            close_brace
        };

        let span = SourceSpan::new(
            self.tokens[start].span.start(),
            self.tokens[semicolon].span.end(),
        )?;
        self.cursor = semicolon + 1;

        Ok(Some(SyntaxItem::Declaration(ShaderDeclaration::new(
            DeclarationKind::Struct,
            None,
            None,
            name,
            None,
            None,
            span,
        ))))
    }

    /// Finds a candidate function parameter opener before a top-level
    /// terminator.
    fn find_top_level_left_paren_before_terminator(&self, start: usize) -> Option<usize> {
        let mut index = start;
        while index < self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => return Some(index),
                TokenKind::Semicolon | TokenKind::LeftBrace | TokenKind::RightBrace => return None,
                _ => index += 1,
            }
        }
        None
    }

    /// Finds the previous non-comment token before `before`.
    fn previous_non_comment(&self, before: usize) -> Option<usize> {
        self.tokens
            .iter()
            .take(before)
            .enumerate()
            .rev()
            .find_map(|(index, token)| (!token.kind.is_comment()).then_some(index))
    }

    /// Finds the next non-comment token at or after `start`.
    fn next_non_comment(&self, start: usize) -> Option<usize> {
        self.tokens
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, token)| (!token.kind.is_comment()).then_some(index))
    }

    /// Extracts the qualifier, type name, and identifier from a declaration
    /// prefix.
    fn declaration_head(&self, start: usize, semicolon: usize) -> DeclarationHead<'src> {
        let mut index = start;
        let mut qualifier = None;
        let mut layout = None;
        let mut type_name = None;
        let mut name = None;

        while index < semicolon {
            match self.tokens[index].kind {
                TokenKind::Identifier("layout") => {
                    let layout_end = self.skip_layout_qualifier(index, semicolon);
                    layout = self.layout_qualifier(index, layout_end);
                    index = layout_end;
                    continue;
                }
                TokenKind::Identifier("uniform") if qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Uniform);
                }
                TokenKind::Identifier("attribute") if qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Attribute);
                }
                TokenKind::Identifier("varying") if qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Varying);
                }
                TokenKind::Identifier("in") if qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::In);
                }
                TokenKind::Identifier("out") if qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Out);
                }
                TokenKind::Identifier(text)
                    if qualifier.is_none()
                        && !self.tokens[index].kind.is_declaration_modifier() =>
                {
                    type_name = Some(text);
                }
                TokenKind::Identifier(text) => {
                    if self.tokens[index].kind.is_declaration_modifier() {
                        // Skip precision/interpolation/auxiliary qualifiers.
                    } else if type_name.is_none() {
                        type_name = Some(text);
                    } else {
                        name = Some(text);
                        break;
                    }
                }
                TokenKind::Punctuation('=') | TokenKind::LeftBrace | TokenKind::Comma => break,
                _ => {}
            }

            index += 1;
        }

        DeclarationHead {
            qualifier,
            layout,
            type_name,
            name,
            array_suffix: name.and_then(|_name| self.array_suffix_after(index, semicolon)),
        }
    }

    /// Returns a typed layout qualifier fact for a skipped layout range.
    fn layout_qualifier(&self, start: usize, end: usize) -> Option<DeclarationLayout<'src>> {
        if end <= start {
            return None;
        }

        SourceSpan::new(
            self.tokens[start].span.start(),
            self.tokens[end - 1].span.end(),
        )
        .ok()
        .map(|span| DeclarationLayout {
            source: self.context.slice(span),
        })
    }

    /// Returns the array suffix immediately after a declaration name.
    fn array_suffix_after(
        &self,
        name_index: usize,
        semicolon: usize,
    ) -> Option<DeclarationArraySuffix<'src>> {
        let open = self.next_non_comment(name_index + 1)?;
        if open >= semicolon || !matches!(self.tokens[open].kind, TokenKind::Punctuation('[')) {
            return None;
        }

        let close = self.find_array_suffix_end(open, semicolon)?;
        SourceSpan::new(
            self.tokens[open].span.start(),
            self.tokens[close].span.end(),
        )
        .ok()
        .map(|span| DeclarationArraySuffix {
            source: self.context.slice(span),
        })
    }

    /// Finds the closing bracket for a simple declaration array suffix.
    fn find_array_suffix_end(&self, open: usize, semicolon: usize) -> Option<usize> {
        let mut index = open + 1;
        while index < semicolon {
            if matches!(self.tokens[index].kind, TokenKind::Punctuation(']')) {
                return Some(index);
            }
            if matches!(
                self.tokens[index].kind,
                TokenKind::Comma | TokenKind::Punctuation('=')
            ) {
                return None;
            }
            index += 1;
        }
        None
    }

    /// Skips over a `layout(...)` qualifier when it appears in a declaration
    /// prefix.
    fn skip_layout_qualifier(&self, index: usize, semicolon: usize) -> usize {
        let Some(next) = self.next_non_comment(index + 1) else {
            return index + 1;
        };
        if next >= semicolon || !matches!(self.tokens[next].kind, TokenKind::LeftParen) {
            return index + 1;
        }

        self.find_matching_paren(next)
            .map_or(index + 1, |close| close + 1)
    }

    /// Finds the closing parenthesis for an opening parenthesis token.
    fn find_matching_paren(&self, open: usize) -> ShaderResult<usize> {
        self.find_balanced(
            open,
            TokenKindMatcher::LeftParen,
            TokenKindMatcher::RightParen,
        )
    }

    /// Finds the closing brace for an opening brace token.
    fn find_matching_brace(&self, open: usize) -> ShaderResult<usize> {
        self.find_balanced(
            open,
            TokenKindMatcher::LeftBrace,
            TokenKindMatcher::RightBrace,
        )
    }

    /// Finds the matching close delimiter for a balanced token pair.
    fn find_balanced(
        &self,
        open: usize,
        open_matcher: TokenKindMatcher,
        close_matcher: TokenKindMatcher,
    ) -> ShaderResult<usize> {
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(open) {
            if open_matcher.matches(token.kind) {
                depth += 1;
            } else if close_matcher.matches(token.kind) {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(index);
                }
            }
        }

        Err(crate::ShaderError::Parse {
            diagnostics: vec![
                ShaderDiagnostic::new("unbalanced shader delimiter")
                    .with_span(self.tokens[open].span),
            ]
            .into_boxed_slice(),
        })
    }

    /// Finds the first matching token before the next semicolon.
    fn find_token(&self, start: usize, matcher: TokenKindMatcher) -> Option<usize> {
        self.tokens
            .iter()
            .enumerate()
            .skip(start)
            .take_while(|(_, token)| !matches!(token.kind, TokenKind::Semicolon))
            .find_map(|(index, token)| matcher.matches(token.kind).then_some(index))
    }
}

/// Parsed declaration header fields used to classify top-level declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeclarationHead<'src> {
    /// Recognized interface qualifier, when present.
    qualifier: Option<TopLevelQualifier>,
    /// Leading layout qualifier, when present.
    layout: Option<DeclarationLayout<'src>>,
    /// Borrowed declaration type token, when known.
    type_name: Option<&'src str>,
    /// Borrowed declaration identifier token, when known.
    name: Option<&'src str>,
    /// Array suffix on the declared identifier, when present.
    array_suffix: Option<DeclarationArraySuffix<'src>>,
}

/// Delimiter token categories used by balanced-token searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKindMatcher {
    /// Matches `{`.
    LeftBrace,
    /// Matches `}`.
    RightBrace,
    /// Matches `(`.
    LeftParen,
    /// Matches `)`.
    RightParen,
}

impl TokenKindMatcher {
    /// Returns whether `kind` matches this delimiter category.
    const fn matches(self, kind: TokenKind<'_>) -> bool {
        matches!(
            (self, kind),
            (Self::LeftBrace, TokenKind::LeftBrace)
                | (Self::RightBrace, TokenKind::RightBrace)
                | (Self::LeftParen, TokenKind::LeftParen)
                | (Self::RightParen, TokenKind::RightParen)
        )
    }
}
