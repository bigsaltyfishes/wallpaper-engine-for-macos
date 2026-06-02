//! Parsed module and top-level syntax items.

use smol_str::SmolStr;

use super::{
    FunctionCall, FunctionDecl, InterfaceUseFacts, InterfaceUseQuery, PreprocessorDirective,
    ShaderAnnotation, ShaderDeclaration, ShaderSourceText, interface_use::InterfaceReference,
};
use crate::{
    ShaderResult, ShaderStageKind, SourceSpan,
    tokenizer::{TokenCursor, TokenStream, TypedToken, TypedTokenFacts},
};

/// Top-level shader syntax item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxItem<'src> {
    /// Shader interface, struct, or loose top-level declaration.
    Declaration(ShaderDeclaration<'src>),
    /// Function signature plus opaque balanced body span.
    Function(FunctionDecl<'src>),
    /// Preprocessor directive line.
    Directive(PreprocessorDirective<'src>),
    /// Wallpaper Engine metadata annotation.
    Annotation(ShaderAnnotation),
    /// Source range skipped by the lightweight parser.
    Opaque(SourceSpan),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_function_calls_continue_after_unmatched_candidate() {
        let module = ShaderModule::parse(ShaderStageKind::Fragment, "broken(1.0; later(2.0)")
            .expect("module parses");

        let calls = module.function_calls().collect::<Vec<_>>();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name(), "later");
        assert_eq!(calls[0].argument_count(), 1);
    }
}

/// Parsed source module for one shader stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderModule<'src> {
    /// Shader stage represented by this parsed source.
    stage: ShaderStageKind,
    /// Typed source view used by all spans in this module.
    source: ShaderSourceText<'src>,
    /// Lexed tokens retained as immutable parse output.
    tokens: TokenStream,
    /// Reusable facts derived from the retained token stream.
    token_facts: TypedTokenFacts,
    /// Top-level syntax items in source order.
    items: Vec<SyntaxItem<'src>>,
}

impl<'src> ShaderModule<'src> {
    /// Creates a shader module from parsed items.
    #[must_use]
    pub fn new(
        stage: ShaderStageKind,
        source: ShaderSourceText<'src>,
        tokens: TokenStream,
        items: Vec<SyntaxItem<'src>>,
    ) -> Self {
        let token_facts = tokens.facts();
        Self {
            stage,
            source,
            tokens,
            token_facts,
            items,
        }
    }

    /// Returns the shader stage represented by this module.
    #[must_use]
    pub const fn stage(&self) -> ShaderStageKind {
        self.stage
    }

    /// Returns the typed shader source parsed into this module.
    #[must_use]
    pub const fn source(&self) -> ShaderSourceText<'src> {
        self.source
    }

    /// Borrows the source text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: SourceSpan) -> &'src str {
        self.source.slice(span)
    }

    /// Returns the span covering the full source text.
    ///
    /// # Errors
    ///
    /// Returns an error when the source range cannot be represented as a
    /// [`SourceSpan`].
    pub fn source_span(&self) -> ShaderResult<SourceSpan> {
        SourceSpan::new(0, self.source.as_str().len())
    }

    /// Returns top-level syntax items in source order.
    #[must_use]
    pub fn items(&self) -> &[SyntaxItem<'src>] {
        &self.items
    }

    /// Returns the lexed source token stream.
    #[must_use]
    pub const fn token_stream(&self) -> &TokenStream {
        &self.tokens
    }

    /// Returns cached reusable facts derived from the token stream.
    #[must_use]
    pub const fn token_facts(&self) -> &TypedTokenFacts {
        &self.token_facts
    }

    /// Iterates syntactic function calls in module source order.
    pub fn function_calls(&self) -> impl Iterator<Item = FunctionCall> + '_ {
        let tokens = self.tokens.cursor();
        self.token_facts
            .calls()
            .iter()
            .filter_map(move |call| FunctionCall::from_fact(tokens, call))
    }

    /// Returns parsed function declarations in source order.
    #[must_use]
    pub fn functions(&self) -> ModuleFunctions<'_, 'src> {
        ModuleFunctions { items: &self.items }
    }

    /// Extracts usage facts for one stage interface declaration.
    #[must_use]
    pub fn interface_use_facts(&self, query: InterfaceUseQuery<'src>) -> InterfaceUseFacts {
        let tokens = self.tokens.cursor();
        let references = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                if token.span().start() < query.declaration_span().end()
                    && token.span().end() > query.declaration_span().start()
                {
                    return None;
                }
                if !matches!(token.kind(), TypedToken::Identifier(name) if name == query.name()) {
                    return None;
                }
                let Some(dot) = tokens.next_non_comment(index + 1) else {
                    return Some(Self::plain_interface_reference(tokens, index));
                };
                if !tokens[dot].kind().is_member_access_operator() {
                    return Some(Self::plain_interface_reference(tokens, index));
                }
                let Some(field) = tokens.next_non_comment(dot + 1) else {
                    return Some(InterfaceReference::PlainRead);
                };
                let TypedToken::Identifier(field) = tokens[field].kind() else {
                    return Some(InterfaceReference::PlainRead);
                };
                Some(InterfaceReference::Swizzle {
                    required_width: field
                        .bytes()
                        .try_fold(0, |width, component| {
                            match component {
                                b'x' | b'r' | b's' => Some(1),
                                b'y' | b'g' | b't' => Some(2),
                                b'z' | b'b' | b'p' => Some(3),
                                b'w' | b'a' | b'q' => Some(4),
                                _ => None,
                            }
                            .map(|index| width.max(index))
                        })
                        .unwrap_or(query.binding_width()),
                })
            })
            .collect();
        InterfaceUseFacts {
            name: SmolStr::new(query.name()),
            declaration_span: query.declaration_span(),
            references,
        }
    }

    /// Returns the first declarator name in `declaration` when the parser did
    /// not classify it directly.
    #[must_use]
    pub fn first_declarator_name(&self, declaration: &ShaderDeclaration<'src>) -> Option<SmolStr> {
        self.tokens
            .cursor()
            .first_declarator_name(declaration.span())
            .map(SmolStr::new)
    }

    /// Classifies a whole-variable interface reference.
    fn plain_interface_reference(tokens: TokenCursor<'_>, index: usize) -> InterfaceReference {
        if tokens
            .next_non_comment(index + 1)
            .is_some_and(|next| tokens[next].kind().is_simple_assignment_operator())
        {
            InterfaceReference::PlainAssignment
        } else {
            InterfaceReference::PlainRead
        }
    }
}

/// View over parsed module function declarations.
#[derive(Clone, Copy, Debug)]
pub struct ModuleFunctions<'module, 'src> {
    /// Module syntax items.
    items: &'module [SyntaxItem<'src>],
}

impl<'module, 'src> ModuleFunctions<'module, 'src> {
    /// Iterates parsed function declarations.
    pub fn iter(self) -> impl Iterator<Item = &'module FunctionDecl<'src>> {
        self.items.iter().filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        })
    }
}
