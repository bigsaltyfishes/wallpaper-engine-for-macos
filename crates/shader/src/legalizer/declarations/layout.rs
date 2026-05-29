//! Resource layout declaration helpers.

use std::collections::BTreeSet;

use super::{
    super::tokens::{BalancedTokens, TokenSearch},
    resources::{TextureDeclaration, UniformMember},
};
use crate::{
    ShaderError, ShaderResult, ShaderStageKind, SourceSpan,
    layout::DescriptorBinding,
    lexer::{Token, TokenKind},
    syntax::{ShaderDeclaration, ShaderModule},
};

/// Program-level descriptor resource layout edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StageResourceLayout<'src> {
    /// Descriptor binding assigned to the generated `GlobalUniforms` block.
    pub(crate) uniform_block_binding: Option<u32>,
    /// Program-level resource bindings kept in source by legalization.
    pub(crate) reserved_bindings: BTreeSet<u32>,
    /// Program-level descriptor assignments for split texture declarations.
    texture_bindings: Vec<StageTextureResourceBinding<'src>>,
    /// Program-wide uniform members emitted by every stage.
    pub(crate) uniform_members: Vec<UniformMember<'src>>,
}

impl<'src> StageResourceLayout<'src> {
    /// Adds a program-level binding assignment for one split texture
    /// declaration.
    pub(crate) fn push_texture_binding(
        &mut self,
        stage: ShaderStageKind,
        name: &'src str,
        texture_binding: u32,
        sampler_binding: u32,
    ) {
        self.texture_bindings.push(StageTextureResourceBinding {
            stage,
            name,
            texture_binding,
            sampler_binding,
        });
    }

    /// Creates a resource allocator seeded with program-level reservations.
    pub(super) fn plan(&self) -> ResourceLayoutPlan {
        ResourceLayoutPlan::from(self)
    }

    /// Returns the program assignment for a split texture in this stage.
    pub(super) fn binding_for_texture(
        &self,
        stage: ShaderStageKind,
        name: &str,
    ) -> Option<StageTextureResourceBinding<'src>> {
        self.texture_bindings
            .iter()
            .copied()
            .find(|binding| binding.matches(stage, name))
    }
}

impl From<&StageResourceLayout<'_>> for ResourceLayoutPlan {
    fn from(layout: &StageResourceLayout<'_>) -> Self {
        let mut plan = Self::default();
        if let Some(binding) = layout.uniform_block_binding {
            let _inserted = plan.used.insert(binding);
        }
        plan.used.extend(layout.reserved_bindings.iter().copied());
        plan.used.extend(
            layout
                .texture_bindings
                .iter()
                .flat_map(|binding| [binding.texture_binding, binding.sampler_binding]),
        );
        plan
    }
}

/// Program-level descriptor assignment for one stage split texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StageTextureResourceBinding<'src> {
    /// Stage containing the source declaration.
    stage: ShaderStageKind,
    /// Source texture variable name.
    name: &'src str,
    /// Descriptor binding assigned to the generated texture handle.
    pub(super) texture_binding: u32,
    /// Descriptor binding assigned to the generated sampler.
    pub(super) sampler_binding: u32,
}

impl StageTextureResourceBinding<'_> {
    /// Returns whether this assignment applies to a stage texture declaration.
    fn matches(self, stage: ShaderStageKind, name: &str) -> bool {
        self.stage == stage && self.name == name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Array suffix extracted from a source declaration.
pub(super) struct ArraySuffix<'module, 'src> {
    /// Parsed shader module containing the declaration.
    pub(super) module: &'module ShaderModule<'src>,
    /// Declaration whose suffix is inspected.
    pub(super) declaration: &'module ShaderDeclaration<'src>,
}

impl<'src> ArraySuffix<'_, 'src> {
    /// Returns source text for `[N]` after a top-level declaration name.
    pub(super) fn source(self) -> ShaderResult<Option<&'src str>> {
        let module = self.module;
        let declaration = self.declaration;
        let Some(name) = declaration.name() else {
            return Ok(None);
        };
        let tokens = module.tokens();
        let Some(name_index) = tokens.iter().position(|token| {
            token.span.start() >= declaration.span().start()
                && token.span.end() <= declaration.span().end()
                && matches!(token.kind, TokenKind::Identifier(text) if text == name)
        }) else {
            return Ok(None);
        };
        let Some(open) = TokenSearch::new(tokens).next_non_comment(name_index + 1) else {
            return Ok(None);
        };
        if !matches!(tokens[open].kind, TokenKind::Punctuation('[')) {
            return Ok(None);
        }
        let Some(close) = BalancedTokens::new(tokens).matching_punctuation(open, '[', ']') else {
            return Ok(None);
        };
        if tokens[close].span.end() > declaration.span().end() {
            return Ok(None);
        }
        Ok(Some(module.slice(SourceSpan::new(
            tokens[open].span.start(),
            tokens[close].span.end(),
        )?)))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Extractor for explicit `layout(binding = N)` qualifiers.
pub(super) struct ExplicitLayoutBinding {
    /// Parsed descriptor binding.
    pub(super) binding: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source data needed to parse a declaration layout qualifier.
pub(super) struct DeclarationLayoutSource<'module, 'src> {
    /// Parsed shader module containing the declaration.
    pub(super) module: &'module ShaderModule<'src>,
    /// Declaration whose layout qualifiers are inspected.
    pub(super) declaration: &'module ShaderDeclaration<'src>,
}

impl TryFrom<DeclarationLayoutSource<'_, '_>> for ExplicitLayoutBinding {
    type Error = ();

    fn try_from(source: DeclarationLayoutSource<'_, '_>) -> Result<Self, Self::Error> {
        let module = source.module;
        let declaration = source.declaration;
        let declaration_span = declaration.span();
        let tokens = module.tokens();
        let start = tokens
            .iter()
            .position(|token| token.span.start() >= declaration_span.start())
            .ok_or(())?;
        let end = tokens
            .iter()
            .position(|token| token.span.end() > declaration_span.end())
            .unwrap_or(tokens.len());
        let mut index = start;
        while index < end {
            if !matches!(tokens[index].kind, TokenKind::Identifier("layout")) {
                index += 1;
                continue;
            }
            let open = TokenSearch::new(tokens)
                .next_non_comment(index + 1)
                .ok_or(())?;
            if open >= end || !matches!(tokens[open].kind, TokenKind::LeftParen) {
                index += 1;
                continue;
            }
            let close = BalancedTokens::new(tokens)
                .matching_right_paren(open)
                .ok_or(())?;
            if close > end {
                return Err(());
            }
            if let Some(binding) = (LayoutQualifierRange {
                tokens,
                start: open + 1,
                end: close,
            }
            .binding())
            {
                return Ok(Self { binding });
            }
            index = close + 1;
        }
        Err(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token range inside one `layout(...)` qualifier.
pub(super) struct LayoutQualifierRange<'module, 'src> {
    /// Tokens containing the qualifier body.
    tokens: &'module [Token<'src>],
    /// First token inside the layout parentheses.
    start: usize,
    /// Closing parenthesis token index.
    end: usize,
}

impl LayoutQualifierRange<'_, '_> {
    /// Returns the `binding` value declared in this qualifier.
    fn binding(self) -> Option<u32> {
        let search = TokenSearch::new(self.tokens);
        for index in self.start..self.end {
            if !matches!(self.tokens[index].kind, TokenKind::Identifier("binding")) {
                continue;
            }
            let equals = search.next_non_comment(index + 1)?;
            if equals >= self.end
                || !matches!(self.tokens[equals].kind, TokenKind::Punctuation('='))
            {
                continue;
            }
            let value = search.next_non_comment(equals + 1)?;
            if value >= self.end {
                continue;
            }
            let TokenKind::Number(text) = self.tokens[value].kind else {
                continue;
            };
            return text.parse::<u32>().ok();
        }
        None
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Descriptor binding allocator that reserves texture-suffixed bindings.
pub(super) struct ResourceLayoutPlan {
    /// Binding numbers already reserved or allocated.
    used: BTreeSet<u32>,
    /// Encoded source texture bindings keyed by descriptor binding.
    encoded_textures: std::collections::BTreeMap<u32, String>,
}

impl ResourceLayoutPlan {
    /// Reserves one already assigned binding.
    pub(super) fn reserve_binding(&mut self, binding: u32) -> ShaderResult<()> {
        if self.used.insert(binding) {
            Ok(())
        } else {
            Err(ShaderError::invalid_request(
                "descriptor binding is already reserved",
            ))
        }
    }

    /// Reserves bindings encoded by sampler names before allocating other
    /// resources.
    pub(super) fn reserve_texture_bindings<'src>(
        &mut self,
        stage: ShaderStageKind,
        textures: impl Iterator<Item = TextureDeclaration<'src>>,
    ) -> ShaderResult<()> {
        for texture in textures {
            if let Some(binding) = texture.texture_binding(stage)? {
                if let Some(previous_name) = self.encoded_textures.get(&binding) {
                    return Err(ShaderError::Legalize {
                        diagnostics: Box::new([texture.duplicate_binding_diagnostic(
                            stage,
                            previous_name,
                            binding,
                        )]),
                    });
                }
                let previous = self
                    .encoded_textures
                    .insert(binding, texture.name.to_owned());
                debug_assert!(previous.is_none());
                let _inserted = self.used.insert(binding);
            }
        }
        Ok(())
    }

    /// Allocates the lowest unused descriptor binding in set zero.
    pub(super) fn allocate(&mut self) -> ShaderResult<DescriptorBinding> {
        let mut binding = 0u32;
        while self.used.contains(&binding) {
            binding += 1;
        }
        let _inserted = self.used.insert(binding);
        DescriptorBinding::new(0, binding)
    }
}
