//! Token-backed legalizer syntax helpers.

use crate::{
    ShaderResult, SourceSpan,
    syntax::{PreprocessorDirective, ShaderModule},
    tokenizer::{Token, TokenStream, TypedToken},
};

impl<'src> PreprocessorDirective<'src> {
    /// Lexes the replacement body of a `#define` directive into source-mapped
    /// tokens, when the directive has a non-empty body.
    pub(crate) fn define_body_tokens_in(
        self,
        module: &ShaderModule<'src>,
    ) -> ShaderResult<Option<TokenStream>> {
        let Some(parts) = self.define_parts().ok().flatten() else {
            return Ok(None);
        };
        if !parts.has_explicit_value() {
            return Ok(None);
        }
        let body = parts.value().as_str();
        let text = self.text_in(module);
        let Some(body_start) = parts.value_offset_in(text) else {
            return Ok(None);
        };

        let source_offset = self.span().start() + body_start;
        let tokens = TokenStream::lex(body)?
            .into_owned()
            .into_iter()
            .map(|token| {
                SourceSpan::new(
                    source_offset + token.span().start(),
                    source_offset + token.span().end(),
                )
                .map(|span| Token::new(token.kind().clone(), span))
            })
            .collect::<ShaderResult<Vec<_>>>()?;
        Ok(Some(TokenStream::new(tokens)))
    }
}

impl<'src> ShaderModule<'src> {
    /// Returns whether any token sequence writes to the target input.
    #[must_use]
    pub(crate) fn writes_stage_input(&self, name: &'src str) -> bool {
        StageInputWrites { module: self, name }.any()
    }
}

/// Scanner for assignments and increments that mutate one stage input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StageInputWrites<'module, 'src> {
    /// Module that owns the token stream.
    module: &'module ShaderModule<'src>,
    /// Stage input identifier to inspect.
    name: &'src str,
}

impl StageInputWrites<'_, '_> {
    /// Returns whether any token sequence writes to the target input.
    pub(super) fn any(self) -> bool {
        let tokens = self.module.token_stream().cursor();
        tokens
            .iter()
            .enumerate()
            .any(|(index, token)| self.writes_at(index, token))
    }

    /// Returns whether the identifier at `index` starts a write expression.
    fn writes_at(self, index: usize, token: &Token) -> bool {
        let tokens = self.module.token_stream().cursor();
        if !matches!(token.kind(), TypedToken::Identifier(text) if text == self.name) {
            return false;
        }

        if let Some(previous) = tokens.previous_non_comment(index)
            && tokens[previous].kind().is_increment_operator()
        {
            return true;
        }

        let Some(next) = tokens.next_non_comment(index + 1) else {
            return false;
        };
        self.tail_writes(next)
    }

    /// Returns whether the tail is assignment-like.
    fn tail_writes(self, start: usize) -> bool {
        let tokens = self.module.token_stream().cursor();
        let mut index = start;
        loop {
            match tokens[index].kind() {
                kind if kind.is_assignment_operator() || kind.is_increment_operator() => {
                    return true;
                }
                kind if kind.is_member_access_operator() => {
                    let Some(next) = tokens.next_non_comment(index + 1) else {
                        return false;
                    };
                    if !matches!(tokens[next].kind(), TypedToken::Identifier(_)) {
                        return false;
                    }
                    let Some(after) = tokens.next_non_comment(next + 1) else {
                        return false;
                    };
                    index = after;
                }
                kind if kind.is_left_square() => {
                    let Some(close) = tokens.matching_right_square(index) else {
                        return false;
                    };
                    let Some(after) = tokens.next_non_comment(close + 1) else {
                        return false;
                    };
                    index = after;
                }
                _ => return false,
            }
        }
    }
}
