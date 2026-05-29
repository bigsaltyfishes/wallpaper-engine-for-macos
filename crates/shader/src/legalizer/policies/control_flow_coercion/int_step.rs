use super::statements::Statement;
use crate::{
    SourceSpan,
    legalizer::{Fixup, FunctionCall, LocalDeclaration, TokenSearch},
    lexer::{Token, TokenKind},
};

/// Integer variable initialized from GLSL `step`, whose return type is float.
pub(super) struct IntStepInitializer<'tokens, 'src> {
    /// Statement being inspected.
    pub(super) statement: Statement<'tokens, 'src>,
}

impl IntStepInitializer<'_, '_> {
    /// Emits non-overlapping structural edits for int declarations initialized
    /// by `step`.
    pub(super) fn fixups(self) -> Vec<Fixup> {
        let tokens = self.statement.tokens;
        let Some(declarations) = self.statement.declaration_declarators("int") else {
            return Vec::new();
        };
        let mut parts = Vec::new();
        let mut step_count = 0usize;
        for declaration in declarations {
            let is_step = IntStepDeclarator {
                declaration,
                is_step: false,
            }
            .uses_step(tokens);
            if is_step {
                step_count += 1;
            }
            parts.push(IntStepDeclarator {
                declaration,
                is_step,
            });
        }
        if step_count == 0 {
            return Vec::new();
        }
        if step_count == parts.len() {
            return vec![Fixup::replace(parts[0].declaration.type_span(), "float")];
        }

        let mut fixups = Vec::new();
        if parts.first().is_some_and(|part| part.is_step) {
            fixups.push(Fixup::replace(parts[0].declaration.type_span(), "float"));
        }
        for pair in parts.windows(2) {
            let previous = pair[0];
            let next = pair[1];
            if previous.ty() == next.ty() {
                continue;
            }
            let Some(separator) = previous.declaration.initializer_separator(tokens) else {
                continue;
            };
            if !matches!(tokens[separator].kind, TokenKind::Comma) {
                continue;
            }
            let Ok(span) = SourceSpan::new(
                tokens[separator].span.start(),
                tokens[next.declaration.name_index()].span.start(),
            ) else {
                continue;
            };
            let mut qualifiers = String::new();
            for token in tokens
                .iter()
                .take(next.declaration.type_index())
                .skip(self.statement.start)
                .filter(|token| !token.kind.is_comment())
            {
                if !token.kind.is_declaration_modifier() {
                    qualifiers.clear();
                    break;
                }
                if let TokenKind::Identifier(text) = token.kind {
                    qualifiers.push_str(text);
                    qualifiers.push(' ');
                }
            }
            fixups.push(Fixup::replace(
                span,
                format!(";\n{qualifiers}{} ", next.ty()),
            ));
        }
        fixups
    }
}

/// One int declarator and whether it must become float.
#[derive(Clone, Copy)]
struct IntStepDeclarator<'src> {
    /// Parsed declarator.
    declaration: LocalDeclaration<'src>,
    /// Whether this declarator is initialized by `step`.
    is_step: bool,
}

impl IntStepDeclarator<'_> {
    /// Returns the type spelling after applying this declarator repair.
    fn ty(self) -> &'static str {
        if self.is_step { "float" } else { "int" }
    }

    /// Returns whether this int declarator is initialized from `step`.
    fn uses_step(self, tokens: &[Token<'_>]) -> bool {
        let Some(initializer) = self.declaration.initializer(tokens) else {
            return false;
        };
        let name = initializer.start();
        let Some(open) = TokenSearch::new(tokens).next_non_comment(name + 1) else {
            return false;
        };
        let close = initializer.end();
        if open >= close
            || !matches!(tokens[open].kind, TokenKind::LeftParen)
            || !matches!(tokens[close].kind, TokenKind::RightParen)
        {
            return false;
        }
        let call = FunctionCall {
            tokens,
            name_index: name,
            open_index: open,
            close_index: close,
        };
        call.name() == "step" && call.argument_count() == 2
    }
}
