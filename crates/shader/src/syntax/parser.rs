//! Cursor-based syntax parser implementation.

use smol_str::SmolStr;

use super::{
    FunctionDecl, FunctionDeclSpans, ParsingContext, PreprocessorDirective, ShaderAnnotation,
    ShaderDeclaration, ShaderModule, SyntaxItem, TopLevelQualifier,
    declaration::{
        DeclarationArraySize, DeclarationArraySuffix, DeclarationKind, DeclarationLayout,
    },
};
use crate::{
    ShaderDiagnostic, ShaderResult, SourceSpan,
    tokenizer::{KeywordType, LiteralValue, TokenCursor, TokenStream, TypedToken},
};

/// Cursor-based parser over the borrowed token stream for one source.
pub(super) struct Parser<'context, 'src> {
    /// Owning parse context that provides stage, source, and token storage.
    pub context: &'context ParsingContext<'src>,
    /// Borrowed tokens being parsed in source order.
    pub tokens: TokenCursor<'context>,
    /// Current token offset within `tokens`.
    pub cursor: usize,
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
            TokenStream::new(self.context.token_stream().clone().into_owned()),
            items,
        ))
    }

    /// Parses the next top-level token sequence into a syntax item.
    fn parse_next_item(&mut self) -> ShaderResult<Option<SyntaxItem<'src>>> {
        let token = &self.tokens[self.cursor];
        match token.kind() {
            TypedToken::Annotation(_) => {
                let text = self.context.slice(token.span());
                self.cursor += 1;
                Ok(Some(SyntaxItem::Annotation(
                    ShaderAnnotation::from_token_text(text, token.span()),
                )))
            }
            TypedToken::Directive(_) => {
                let text = self.context.slice(token.span());
                self.cursor += 1;
                Ok(Some(SyntaxItem::Directive(
                    PreprocessorDirective::from_token_text(text, token.span()),
                )))
            }
            TypedToken::Keyword(KeywordType::Struct) => self.parse_struct_declaration(),
            TypedToken::Identifier(_) | TypedToken::Keyword(_) | TypedToken::TypeMark(_) => {
                self.parse_identifier_item()
            }
            _ => {
                self.cursor += 1;
                Ok(Some(SyntaxItem::Opaque(token.span())))
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

        let name_token = &self.tokens[name_index];
        let return_type_token = &self.tokens[return_type_index];
        let Some(name) = name_token.kind().identifier_text() else {
            return Ok(None);
        };
        let Some(return_type) = return_type_token.kind().source_text() else {
            return Ok(None);
        };

        let close_paren = self.find_matching_paren(open_paren)?;
        let Some(body_open) = self.next_non_comment(close_paren + 1) else {
            return Ok(None);
        };
        if !matches!(self.tokens[body_open].kind(), TypedToken::LeftBrace) {
            return Ok(None);
        }

        let body_close = self.find_matching_brace(body_open)?;
        let signature = SourceSpan::new(
            self.tokens[start].span().start(),
            self.tokens[close_paren].span().end(),
        )?;
        let parameters = SourceSpan::new(
            self.tokens[open_paren].span().end(),
            self.tokens[close_paren].span().start(),
        )?;
        let parameter_facts = FunctionDecl::parse_parameters(self.tokens, open_paren, close_paren);
        let body = SourceSpan::new(
            self.tokens[body_open].span().start(),
            self.tokens[body_close].span().end(),
        )?;
        let span = SourceSpan::new(
            self.tokens[start].span().start(),
            self.tokens[body_close].span().end(),
        )?;

        self.cursor = body_close + 1;

        Ok(Some(FunctionDecl::with_spans(
            return_type,
            name,
            FunctionDeclSpans {
                name: name_token.span(),
                parameters,
                signature,
                body,
                span,
            },
            parameter_facts,
        )))
    }

    /// Parses tokens through the next semicolon as a top-level declaration.
    fn parse_semicolon_declaration(&mut self) -> ShaderResult<ShaderDeclaration<'src>> {
        let start = self.cursor;
        let mut end = start;

        while end < self.tokens.len() {
            if matches!(self.tokens[end].kind(), TypedToken::Semicolon) {
                let declaration = self.declaration_from_range(start, end)?;
                self.cursor = end + 1;
                return Ok(declaration);
            }

            if matches!(
                self.tokens[end].kind(),
                TypedToken::LeftBrace | TypedToken::RightBrace
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
            self.tokens[start]
                .kind()
                .identifier_text()
                .map(SmolStr::new),
            None,
            None,
            self.tokens[start].span(),
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
                self.tokens[start].span().start(),
                self.tokens[semicolon].span().end(),
            )?,
        ))
    }

    /// Parses a struct declaration and its balanced body.
    fn parse_struct_declaration(&mut self) -> ShaderResult<Option<SyntaxItem<'src>>> {
        let start = self.cursor;
        let name = self
            .tokens
            .get(start + 1)
            .and_then(|token| token.kind().identifier_text())
            .map(SmolStr::new);
        let Some(open_brace) = self.find_token(start, TokenKindMatcher::LeftBrace) else {
            return Ok(Some(SyntaxItem::Declaration(
                self.parse_semicolon_declaration()?,
            )));
        };

        let close_brace = self.find_matching_brace(open_brace)?;
        let semicolon = if self
            .tokens
            .get(close_brace + 1)
            .is_some_and(|token| matches!(token.kind(), TypedToken::Semicolon))
        {
            close_brace + 1
        } else {
            close_brace
        };

        let span = SourceSpan::new(
            self.tokens[start].span().start(),
            self.tokens[semicolon].span().end(),
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
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => return Some(index),
                TypedToken::Semicolon | TypedToken::LeftBrace | TypedToken::RightBrace => {
                    return None;
                }
                _ => index += 1,
            }
        }
        None
    }

    /// Finds the previous non-comment token before `before`.
    fn previous_non_comment(&self, before: usize) -> Option<usize> {
        self.tokens.previous_non_comment(before)
    }

    /// Finds the next non-comment token at or after `start`.
    fn next_non_comment(&self, start: usize) -> Option<usize> {
        self.tokens.next_non_comment(start)
    }

    /// Extracts the qualifier, type name, and identifier from a declaration
    /// prefix.
    fn declaration_head(&self, start: usize, semicolon: usize) -> DeclarationHead<'src> {
        let mut index = start;
        let mut qualifier = None;
        let mut layout = None;
        let mut binding_layout = None;
        let mut type_name = None;
        let mut name = None;

        while index < semicolon {
            match self.tokens[index].kind() {
                kind if kind.is_keyword(KeywordType::Layout) => {
                    let layout_end = self.skip_layout_qualifier(index, semicolon);
                    let parsed_layout = self.layout_qualifier(index, layout_end);
                    if binding_layout.is_none()
                        && parsed_layout.is_some_and(|layout| layout.binding().is_some())
                    {
                        binding_layout = parsed_layout;
                    }
                    layout = parsed_layout;
                    index = layout_end;
                    continue;
                }
                kind if kind.is_keyword(KeywordType::Uniform) && qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Uniform);
                }
                kind if kind.is_keyword(KeywordType::Attribute) && qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Attribute);
                }
                kind if kind.is_keyword(KeywordType::Varying) && qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Varying);
                }
                kind if kind.is_keyword(KeywordType::In) && qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::In);
                }
                kind if kind.is_keyword(KeywordType::Out) && qualifier.is_none() => {
                    qualifier = Some(TopLevelQualifier::Out);
                }
                kind if qualifier.is_none()
                    && kind.source_text().is_some()
                    && !kind.is_declaration_modifier() =>
                {
                    type_name = kind.source_text().map(SmolStr::new);
                }
                kind if kind.source_text().is_some() => {
                    if kind.is_declaration_modifier() {
                        // Skip precision/interpolation/auxiliary qualifiers.
                    } else if type_name.is_none() {
                        type_name = kind.source_text().map(SmolStr::new);
                    } else {
                        name = kind.source_text().map(SmolStr::new);
                        break;
                    }
                }
                kind if kind.is_simple_assignment_operator()
                    || matches!(kind, TypedToken::LeftBrace | TypedToken::Comma) =>
                {
                    break;
                }
                _ => {}
            }

            index += 1;
        }

        DeclarationHead {
            qualifier,
            layout: binding_layout.or(layout),
            type_name,
            array_suffix: name
                .as_ref()
                .and_then(|_name| self.array_suffix_after(index, semicolon)),
            name,
        }
    }

    /// Returns a typed layout qualifier fact for a skipped layout range.
    fn layout_qualifier(&self, start: usize, end: usize) -> Option<DeclarationLayout<'src>> {
        if end <= start {
            return None;
        }

        SourceSpan::new(
            self.tokens[start].span().start(),
            self.tokens[end - 1].span().end(),
        )
        .ok()
        .map(|span| DeclarationLayout {
            source: self.context.slice(span),
            set: self.layout_integer_field(start, end, "set"),
            binding: self.layout_integer_field(start, end, "binding"),
        })
    }

    /// Returns an integer field value from a `layout(...)` token range.
    fn layout_integer_field(&self, start: usize, end: usize, field: &str) -> Option<u32> {
        let open = self.next_non_comment(start + 1)?;
        if open >= end || !self.tokens[open].kind().is_left_paren() {
            return None;
        }
        let close = self.find_matching_paren(open).ok()?;
        if close >= end {
            return None;
        }
        self.tokens.integer_field_value(open + 1, close, field)
    }

    /// Returns the array suffix immediately after a declaration name.
    fn array_suffix_after(
        &self,
        name_index: usize,
        semicolon: usize,
    ) -> Option<DeclarationArraySuffix<'src>> {
        let open = self.next_non_comment(name_index + 1)?;
        if open >= semicolon || !self.tokens[open].kind().is_left_square() {
            return None;
        }

        let close = self.find_array_suffix_end(open, semicolon)?;
        SourceSpan::new(
            self.tokens[open].span().start(),
            self.tokens[close].span().end(),
        )
        .ok()
        .map(|span| DeclarationArraySuffix {
            source: self.context.slice(span),
            size: self.array_size_expression(open, close),
        })
    }

    /// Returns a parsed size expression from a declaration array suffix.
    fn array_size_expression(&self, open: usize, close: usize) -> Option<DeclarationArraySize> {
        let value = self.next_non_comment(open + 1)?;
        if value >= close {
            return None;
        }
        if self.next_non_comment(value + 1)? != close {
            return None;
        }
        match self.tokens[value].kind() {
            TypedToken::Literal(LiteralValue::Number(text)) => {
                text.parse::<u32>().ok().map(DeclarationArraySize::Numeric)
            }
            TypedToken::Identifier(identifier) => {
                Some(DeclarationArraySize::MacroIdentifier(identifier.clone()))
            }
            _ => None,
        }
    }

    /// Finds the closing bracket for a simple declaration array suffix.
    fn find_array_suffix_end(&self, open: usize, semicolon: usize) -> Option<usize> {
        let mut index = open + 1;
        while index < semicolon {
            if self.tokens[index].kind().is_right_square() {
                return Some(index);
            }
            if matches!(self.tokens[index].kind(), TypedToken::Comma)
                || self.tokens[index].kind().is_simple_assignment_operator()
            {
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
        if next >= semicolon || !self.tokens[next].kind().is_left_paren() {
            return index + 1;
        }

        self.find_matching_paren(next)
            .map_or(index + 1, |close| close + 1)
    }

    /// Finds the closing parenthesis for an opening parenthesis token.
    fn find_matching_paren(&self, open: usize) -> ShaderResult<usize> {
        self.find_balanced(open, self.tokens.matching_right_paren(open))
    }

    /// Finds the closing brace for an opening brace token.
    fn find_matching_brace(&self, open: usize) -> ShaderResult<usize> {
        self.find_balanced(open, self.tokens.matching_right_brace(open))
    }

    /// Finds the matching close delimiter for a balanced token pair.
    fn find_balanced(&self, open: usize, matched: Option<usize>) -> ShaderResult<usize> {
        if let Some(close) = matched {
            return Ok(close);
        }

        Err(crate::ShaderError::Parse {
            diagnostics: vec![
                ShaderDiagnostic::new("unbalanced shader delimiter")
                    .with_span(self.tokens[open].span()),
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
            .take_while(|(_, token)| !matches!(token.kind(), TypedToken::Semicolon))
            .find_map(|(index, token)| matcher.matches(token.kind()).then_some(index))
    }
}

/// Parsed declaration header fields used to classify top-level declarations.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeclarationHead<'src> {
    /// Recognized interface qualifier, when present.
    qualifier: Option<TopLevelQualifier>,
    /// Leading layout qualifier, when present.
    layout: Option<DeclarationLayout<'src>>,
    /// Declaration type token, when known.
    type_name: Option<SmolStr>,
    /// Declaration identifier token, when known.
    name: Option<SmolStr>,
    /// Array suffix on the declared identifier, when present.
    array_suffix: Option<DeclarationArraySuffix<'src>>,
}

/// Delimiter token categories used by balanced-token searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKindMatcher {
    /// Matches `{`.
    LeftBrace,
}

impl TokenKindMatcher {
    /// Returns whether `kind` matches this delimiter category.
    const fn matches(self, kind: &TypedToken) -> bool {
        matches!((self, kind), (Self::LeftBrace, TypedToken::LeftBrace))
    }
}
