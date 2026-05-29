//! Declarator and type-name scanners.

use super::{
    functions::FunctionParameterList,
    scoped::{DeclarationTail, LocalDeclaration, LocalDeclarationTailStart, NextDeclarator},
};
use crate::{
    SourceSpan,
    lexer::{Token, TokenKind},
};

/// Candidate declaration context in the surrounding token stream.
#[derive(Clone, Copy)]
pub(super) struct DeclarationCandidate<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Candidate type or qualifier token.
    pub(super) start: usize,
}

impl DeclarationCandidate<'_, '_> {
    /// Returns whether the candidate sits inside a function parameter list.
    pub(super) fn is_function_parameter(self) -> bool {
        FunctionParameterList::try_from(self).is_ok()
    }
}

/// Token range for one declarator initializer.
#[derive(Clone, Copy)]
pub struct DeclaratorInitializer {
    /// First non-comment initializer token.
    pub(super) start: usize,
    /// Last non-comment initializer token.
    pub(super) end: usize,
    /// Source span covering the initializer expression.
    pub(super) span: SourceSpan,
}

impl DeclaratorInitializer {
    /// Returns the first initializer token.
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    /// Returns the last initializer token.
    pub(crate) const fn end(self) -> usize {
        self.end
    }

    /// Returns the initializer source span.
    pub(crate) const fn span(self) -> SourceSpan {
        self.span
    }
}

/// Declarators belonging to one local declaration statement.
pub struct DeclarationDeclarators<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Next parsed declarator in the statement.
    next: Option<LocalDeclaration<'src>>,
}

impl<'tokens, 'src> DeclarationDeclarators<'tokens, 'src> {
    /// Creates a declarator iterator from a parsed declaration.
    pub(crate) const fn new(
        tokens: &'tokens [Token<'src>],
        declaration: LocalDeclaration<'src>,
    ) -> Self {
        Self {
            tokens,
            next: Some(declaration),
        }
    }
}

impl<'src> Iterator for DeclarationDeclarators<'_, 'src> {
    type Item = LocalDeclaration<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;
        let next_name = NextDeclarator {
            tokens: self.tokens,
            start: current.name_index() + 1,
            end: current.tail_start().saturating_sub(1),
        }
        .name_index();
        self.next = next_name.and_then(|name_index| {
            let TokenKind::Identifier(name) = self.tokens[name_index].kind else {
                return None;
            };
            Some(LocalDeclaration {
                name,
                ty: current.ty(),
                type_index: current.type_index(),
                name_index,
                tail_start: current.tail_start(),
                declarator_end: DeclarationTail::from(LocalDeclarationTailStart {
                    tokens: self.tokens,
                    start: name_index + 1,
                })
                .declarator_end(current.tail_start())?,
                scope_end: current.scope_end(),
                name_span: self.tokens[name_index].span,
                type_span: current.type_span(),
            })
        });
        Some(current)
    }
}

/// Local declaration type name.
#[derive(Clone, Copy)]
pub struct DeclarationTypeName<'src> {
    /// Source spelling.
    name: &'src str,
}

impl<'src> From<&'src str> for DeclarationTypeName<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}

impl DeclarationTypeName<'_> {
    /// Returns whether this type is one covered by the C++ strategy.
    pub(crate) const fn is_builtin(self) -> bool {
        matches!(
            self.name.as_bytes(),
            b"bool"
                | b"int"
                | b"uint"
                | b"float"
                | b"float1"
                | b"float2"
                | b"float3"
                | b"float4"
                | b"vec2"
                | b"vec3"
                | b"vec4"
                | b"ivec2"
                | b"ivec3"
                | b"ivec4"
                | b"uvec2"
                | b"uvec3"
                | b"uvec4"
                | b"bvec2"
                | b"bvec3"
                | b"bvec4"
                | b"mat2"
                | b"mat3"
                | b"mat4"
                | b"mat2x2"
                | b"mat2x3"
                | b"mat2x4"
                | b"mat3x2"
                | b"mat3x3"
                | b"mat3x4"
                | b"mat4x2"
                | b"mat4x3"
                | b"mat4x4"
        )
    }
}

/// Local declaration type name.
#[derive(Clone, Copy)]
pub struct LocalTypeName<'src> {
    /// Source spelling.
    name: &'src str,
}

impl<'src> From<&'src str> for LocalTypeName<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}

impl LocalTypeName<'_> {
    /// Returns whether this type is one covered by the C++ strategy.
    pub(crate) fn is_local(self) -> bool {
        DeclarationTypeName::from(self.name).is_builtin()
    }
}
