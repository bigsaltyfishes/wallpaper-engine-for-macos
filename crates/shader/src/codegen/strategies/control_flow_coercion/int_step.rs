use super::statements::StatementFixupInput;
use crate::{
    SourceSpan,
    codegen::{Fixup, LocalDeclaration},
    tokenizer::{TokenCursor, TypedToken, TypedTokenFacts},
};

/// Integer variable initialized from GLSL `step`, whose return type is float.
pub(super) struct IntStepInitializer<'statement> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'statement TypedTokenFacts,
}

impl IntStepInitializer<'_> {
    /// Emits non-overlapping structural edits for int declarations initialized
    /// by `step`.
    pub(super) fn fixups(self, tokens: TokenCursor<'_>) -> Vec<Fixup> {
        let declarations = self
            .statement
            .declaration_declarators(tokens, self.token_facts, "int");
        if declarations.is_empty() {
            return Vec::new();
        }
        let mut parts = Vec::new();
        let mut step_count = 0usize;
        for declaration in declarations {
            let is_step = IntStepDeclarator {
                declaration: declaration.clone(),
                is_step: false,
            }
            .uses_step(tokens, self.token_facts);
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
            let previous = &pair[0];
            let next = &pair[1];
            if previous.ty() == next.ty() {
                continue;
            }
            let Some(separator) = previous.declaration.initializer_separator(tokens) else {
                continue;
            };
            if !matches!(tokens[separator].kind(), TypedToken::Comma) {
                continue;
            }
            let Ok(span) = SourceSpan::new(
                tokens[separator].span().start(),
                tokens[next.declaration.name_index()].span().start(),
            ) else {
                continue;
            };
            let mut qualifiers = String::new();
            for token in tokens
                .iter()
                .take(next.declaration.type_index())
                .skip(self.statement.start())
            {
                if !token.kind().is_declaration_modifier() {
                    qualifiers.clear();
                    break;
                }
                if let Some(text) = token.kind().source_text() {
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
#[derive(Clone)]
struct IntStepDeclarator {
    /// Parsed declarator.
    declaration: LocalDeclaration,
    /// Whether this declarator is initialized by `step`.
    is_step: bool,
}

impl IntStepDeclarator {
    /// Returns the type spelling after applying this declarator repair.
    fn ty(&self) -> &'static str {
        if self.is_step { "float" } else { "int" }
    }

    /// Returns whether this int declarator is initialized from `step`.
    fn uses_step(&self, tokens: TokenCursor<'_>, token_facts: &TypedTokenFacts) -> bool {
        let Some(initializer) = self.declaration.initializer(tokens) else {
            return false;
        };
        token_facts
            .call_at_name(initializer.start())
            .filter(|call| {
                call.close_index() == initializer.end()
                    && tokens[call.open_index()].kind().is_left_paren()
                    && tokens[call.close_index()].kind().is_right_paren()
            })
            .is_some_and(|call| call.name() == "step" && call.arguments.len() == 2)
    }
}
