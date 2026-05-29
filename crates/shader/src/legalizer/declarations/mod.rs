//! Top-level declaration planning and generated declaration models.

mod functions;
mod interface;
mod layout;
mod resources;
mod types;

use std::borrow::Cow;

pub use functions::FunctionEntry;
pub use interface::{
    InterfaceDirection, StageInterfaceInitializer, StageInterfaceLayout,
    StageInterfaceLayoutBinding, SynthesizedName, SynthesizedStageInterface,
};
pub use layout::StageResourceLayout;
pub use resources::{FragmentOutput, SamplerType, UniformMember};

use self::{
    functions::{CompatibilityFunctionRequests, FunctionSource},
    interface::{MacroAliasedPositionDeclaration, StageInterface},
    layout::{ArraySuffix, DeclarationLayoutSource, ExplicitLayoutBinding},
    resources::{TextureDeclaration, TextureSampler, UniformBlock},
};
use crate::{
    ShaderError, ShaderResult, ShaderStageKind, SourceSpan,
    layout::{DescriptorBinding, InterfaceLocation, LocationAllocator},
    syntax::{ShaderDeclaration, ShaderModule, SyntaxItem, TopLevelQualifier},
};

/// Planned declaration replacements derived from parsed top-level syntax.
pub struct DeclarationPlan<'src> {
    /// Shader stage that owns the planned declarations.
    stage: ShaderStageKind,
    /// Source declarations and their legalization actions.
    entries: Vec<DeclarationEntry<'src>>,
    /// Parsed functions relevant to builtin collision handling.
    functions: Vec<FunctionEntry<'src>>,
    /// Whether the fragment stage needs an explicit color output.
    fragment_output: bool,
    /// Compatibility helper functions requested by source references.
    compatibility_functions: CompatibilityFunctionRequests,
    /// Program-level stage interface edits supplied by pipeline assembly.
    pub(crate) interface_layout: StageInterfaceLayout<'src>,
    /// Program-level resource layout edits supplied by pipeline assembly.
    pub(crate) resource_layout: StageResourceLayout<'src>,
}

impl<'src> TryFrom<&ShaderModule<'src>> for DeclarationPlan<'src> {
    /// Error type returned when declaration planning cannot compute source
    /// spans.
    type Error = ShaderError;

    /// Builds a declaration plan from parsed top-level shader syntax.
    fn try_from(module: &ShaderModule<'src>) -> Result<Self, Self::Error> {
        let mut entries = Vec::new();
        let mut functions = Vec::new();

        for item in module.items() {
            match item {
                SyntaxItem::Declaration(declaration) => {
                    entries.push(DeclarationEntry {
                        span: declaration.span(),
                        kind: PlannedDeclaration::try_from(DeclarationSource {
                            module,
                            declaration,
                        })?,
                    });
                }
                SyntaxItem::Function(function) => {
                    functions.push(FunctionEntry::from(FunctionSource { module, function }));
                }
                SyntaxItem::Annotation(_) | SyntaxItem::Directive(_) | SyntaxItem::Opaque(_) => {}
            }
        }

        Ok(Self {
            stage: module.stage(),
            entries,
            functions,
            fragment_output: false,
            compatibility_functions: CompatibilityFunctionRequests::default(),
            interface_layout: StageInterfaceLayout::default(),
            resource_layout: StageResourceLayout::default(),
        })
    }
}

impl<'src> DeclarationPlan<'src> {
    /// Assigns descriptor bindings and stage locations to planned declarations.
    pub(super) fn plan_layouts(&mut self) -> ShaderResult<()> {
        let mut inputs = LocationAllocator::default();
        let mut outputs = LocationAllocator::default();
        let mut layout = self.resource_layout.plan();

        layout.reserve_texture_bindings(
            self.stage,
            self.entries.iter().filter_map(|entry| match entry.kind {
                PlannedDeclaration::Texture(texture) => Some(texture),
                PlannedDeclaration::Keep
                | PlannedDeclaration::MacroAliasedPosition(_)
                | PlannedDeclaration::Interface(_)
                | PlannedDeclaration::UniformMember(_) => None,
            }),
        )?;
        let has_stage_uniforms = self
            .entries
            .iter()
            .any(|entry| matches!(entry.kind, PlannedDeclaration::UniformMember(_)));
        if has_stage_uniforms || !self.resource_layout.uniform_members.is_empty() {
            let uniform_binding = self.resource_layout.uniform_block_binding.or_else(|| {
                self.entries.iter().find_map(|entry| match &entry.kind {
                    PlannedDeclaration::UniformMember(member) => member.explicit_binding,
                    PlannedDeclaration::Keep
                    | PlannedDeclaration::MacroAliasedPosition(_)
                    | PlannedDeclaration::Interface(_)
                    | PlannedDeclaration::Texture(_) => None,
                })
            });
            let binding = if let Some(binding) = self.resource_layout.uniform_block_binding {
                DescriptorBinding::new(0, binding)?
            } else if let Some(binding) = uniform_binding {
                layout.reserve_binding(binding)?;
                DescriptorBinding::new(0, binding)?
            } else {
                layout.allocate()?
            };
            if has_stage_uniforms {
                for entry in self
                    .entries
                    .iter_mut()
                    .filter(|entry| matches!(entry.kind, PlannedDeclaration::UniformMember(_)))
                {
                    entry.set_uniform_block_binding(binding);
                }
            }
        }
        for entry in self.entries.iter_mut().filter(|entry| entry.is_texture()) {
            if let Some(assignment) = entry
                .texture_name()
                .and_then(|name| self.resource_layout.binding_for_texture(self.stage, name))
            {
                entry.set_texture_binding(DescriptorBinding::new(0, assignment.texture_binding)?);
                entry.set_texture_sampler_binding(DescriptorBinding::new(
                    0,
                    assignment.sampler_binding,
                )?);
                continue;
            }

            let binding = if let Some(texture_binding) = entry.texture_binding(self.stage)? {
                DescriptorBinding::new(0, texture_binding)?
            } else {
                layout.allocate()?
            };
            entry.set_texture_binding(binding);
        }

        for entry in self.entries.iter_mut().filter(|entry| entry.is_texture()) {
            if !entry.has_texture_sampler_binding() {
                entry.set_texture_sampler_binding(layout.allocate()?);
            }
        }

        let interface_layout = &self.interface_layout;
        for entry in self.entries.iter_mut().filter(|entry| entry.is_interface()) {
            let Some(interface) = entry.interface_mut() else {
                continue;
            };
            let binding = interface_layout.binding_for(interface);
            if let Some(binding) = binding
                && let Some(ty) = binding.ty
            {
                interface.ty = ty;
            }
            let location = binding.map_or_else(
                || match interface.direction {
                    InterfaceDirection::Input => inputs.allocate(),
                    InterfaceDirection::Output => outputs.allocate(),
                },
                |binding| InterfaceLocation::new(binding.location),
            )?;
            interface.location = Some(location);
        }
        self.interface_layout
            .push_synthesized_stage_interfaces(self.stage, &mut self.entries)?;

        Ok(())
    }

    /// Returns original declaration spans that must be removed from source.
    pub(super) fn removed_declarations(&self) -> impl Iterator<Item = SourceSpan> + '_ {
        self.entries
            .iter()
            .filter(|entry| entry.is_removed_from_original())
            .map(|entry| entry.span)
    }

    /// Returns qualifier spans that should be removed while preserving the
    /// source declaration.
    pub(super) fn removed_qualifiers<'plan>(
        &'plan self,
        source: &'plan str,
    ) -> impl Iterator<Item = SourceSpan> + 'plan {
        self.entries
            .iter()
            .filter_map(|entry| entry.removed_qualifier(source))
    }

    /// Iterates mutable stage inputs so write detection can request local
    /// copies.
    pub(crate) fn stage_inputs_mut(&mut self) -> impl Iterator<Item = &mut StageInterface<'src>> {
        self.entries.iter_mut().filter_map(|entry| {
            let PlannedDeclaration::Interface(interface) = &mut entry.kind else {
                return None;
            };
            (interface.direction == InterfaceDirection::Input).then_some(interface)
        })
    }

    /// Iterates interfaces that need statements inserted at the beginning of
    /// `main`.
    pub(super) fn main_prelude_interfaces(&self) -> impl Iterator<Item = &StageInterface<'src>> {
        self.entries.iter().filter_map(|entry| {
            let PlannedDeclaration::Interface(interface) = &entry.kind else {
                return None;
            };
            (interface.local_copy || interface.initializer.is_some()).then_some(interface)
        })
    }

    /// Iterates all planned stage interfaces for generated declaration
    /// emission.
    pub(super) fn stage_interfaces(&self) -> impl Iterator<Item = &StageInterface<'src>> {
        self.entries.iter().filter_map(|entry| match &entry.kind {
            PlannedDeclaration::Interface(interface) => Some(interface),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Texture(_)
            | PlannedDeclaration::UniformMember(_) => None,
        })
    }

    /// Returns the emitted scalar/vector type for a stage interface name.
    pub(crate) fn stage_interface_ty(&self, name: &str) -> Option<&'src str> {
        self.stage_interfaces()
            .find(|interface| interface.name.as_ref() == name)
            .map(|interface| interface.ty)
    }

    /// Builds the generated std140 uniform block when scalar uniforms exist.
    pub(super) fn uniform_block(&self) -> Option<UniformBlock<'src>> {
        if !self.resource_layout.uniform_members.is_empty() {
            let binding = DescriptorBinding::new(0, self.resource_layout.uniform_block_binding?)
                .expect("program resource layout should contain a valid uniform binding");
            return Some(UniformBlock {
                members: self.resource_layout.uniform_members.clone(),
                binding,
            });
        }

        let members: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                PlannedDeclaration::UniformMember(member) => Some(member.clone()),
                PlannedDeclaration::Keep
                | PlannedDeclaration::MacroAliasedPosition(_)
                | PlannedDeclaration::Interface(_)
                | PlannedDeclaration::Texture(_) => None,
            })
            .collect();
        let binding = self.entries.iter().find_map(|entry| match &entry.kind {
            PlannedDeclaration::UniformMember(member) => member.binding,
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Interface(_)
            | PlannedDeclaration::Texture(_) => None,
        })?;
        (!members.is_empty()).then_some(UniformBlock { members, binding })
    }

    /// Iterates texture declarations that need explicit descriptor bindings.
    pub(super) fn textures(&self) -> impl Iterator<Item = TextureDeclaration<'src>> + '_ {
        self.entries.iter().filter_map(|entry| match entry.kind {
            PlannedDeclaration::Texture(texture) => Some(texture),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Interface(_)
            | PlannedDeclaration::UniformMember(_) => None,
        })
    }

    /// Returns whether any source sampler2D declarations were mapped to
    /// separated texture handles.
    pub(crate) fn has_textures(&self) -> bool {
        self.textures().next().is_some()
    }

    /// Iterates generated sampler declarations paired to source textures.
    pub(super) fn texture_samplers(&self) -> impl Iterator<Item = TextureSampler<'src>> + '_ {
        self.textures().filter_map(|texture| {
            Some(TextureSampler {
                texture_name: texture.name,
                binding: texture.sampler_binding?,
            })
        })
    }

    /// Returns the generated sampler name for a source texture declaration.
    pub(crate) fn texture_sampler_name(&self, name: &str) -> Option<String> {
        self.textures()
            .find(|texture| texture.name == name)
            .map(|texture| format!("{}{}", TextureDeclaration::SAMPLER_PREFIX, texture.name))
    }

    /// Marks that generated source needs an explicit fragment output.
    pub(crate) fn require_fragment_output(&mut self) {
        self.fragment_output = true;
    }

    /// Returns whether a generated fragment output is required.
    pub(super) const fn has_fragment_output(&self) -> bool {
        self.fragment_output
    }

    /// Requests generated `clip` overloads.
    pub(crate) fn require_clip_functions(&mut self) {
        self.compatibility_functions.require_clip();
    }

    /// Requests generated `PerformLighting_V1` overloads.
    pub(crate) fn require_perform_lighting_functions(&mut self) {
        self.compatibility_functions.require_perform_lighting();
    }

    /// Emits requested compatibility helper functions.
    pub(super) fn emit_compatibility_functions(&self, output: &mut String) -> ShaderResult<()> {
        self.compatibility_functions.emit(output)
    }

    /// Returns whether the shader declares a user function named `mod`.
    pub(crate) fn has_user_function(&self, name: &str) -> bool {
        self.functions.iter().any(|function| function.name == name)
    }

    /// Iterates user functions named `name` so their declarations can be
    /// renamed.
    pub(crate) fn user_functions<'plan>(
        &'plan self,
        name: &'plan str,
    ) -> impl Iterator<Item = &'plan FunctionEntry<'src>> {
        self.functions
            .iter()
            .filter(move |function| function.name == name)
    }
}

/// Planned action for one original top-level declaration.
struct DeclarationEntry<'src> {
    /// Span of the original declaration in source text.
    span: SourceSpan,
    /// Legalization action associated with the declaration.
    kind: PlannedDeclaration<'src>,
}

impl<'src> DeclarationEntry<'src> {
    /// Returns whether the entry is a stage interface declaration.
    const fn is_interface(&self) -> bool {
        matches!(self.kind, PlannedDeclaration::Interface(_))
    }

    /// Returns whether the entry is a separated texture declaration.
    const fn is_texture(&self) -> bool {
        matches!(self.kind, PlannedDeclaration::Texture(_))
    }

    /// Returns whether the original declaration is replaced by generated
    /// source.
    const fn is_removed_from_original(&self) -> bool {
        matches!(
            self.kind,
            PlannedDeclaration::Interface(_)
                | PlannedDeclaration::Texture(_)
                | PlannedDeclaration::UniformMember(_)
        )
    }

    /// Returns the mutable stage interface declaration, when present.
    fn interface_mut(&mut self) -> Option<&mut StageInterface<'src>> {
        match &mut self.kind {
            PlannedDeclaration::Interface(interface) => Some(interface),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Texture(_)
            | PlannedDeclaration::UniformMember(_) => None,
        }
    }

    /// Stores the descriptor binding on a planned texture.
    fn set_texture_binding(&mut self, binding: DescriptorBinding) {
        if let PlannedDeclaration::Texture(texture) = &mut self.kind {
            texture.binding = Some(binding);
        }
    }

    /// Stores the descriptor binding for the sampler paired with a planned
    /// texture.
    fn set_texture_sampler_binding(&mut self, binding: DescriptorBinding) {
        if let PlannedDeclaration::Texture(texture) = &mut self.kind {
            texture.sampler_binding = Some(binding);
        }
    }

    /// Returns whether the sampler paired with this texture already has an
    /// assigned descriptor binding.
    const fn has_texture_sampler_binding(&self) -> bool {
        match self.kind {
            PlannedDeclaration::Texture(texture) => texture.sampler_binding.is_some(),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Interface(_)
            | PlannedDeclaration::UniformMember(_) => false,
        }
    }

    /// Stores the shared uniform block binding on a planned uniform member.
    fn set_uniform_block_binding(&mut self, binding: DescriptorBinding) {
        if let PlannedDeclaration::UniformMember(member) = &mut self.kind {
            member.binding = Some(binding);
        }
    }

    /// Returns the source texture name for a separated texture declaration.
    fn texture_name(&self) -> Option<&'src str> {
        match self.kind {
            PlannedDeclaration::Texture(texture) => Some(texture.name),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Interface(_)
            | PlannedDeclaration::UniformMember(_) => None,
        }
    }

    /// Returns a texture-suffixed sampler binding when the name encodes one.
    fn texture_binding(&self, stage: ShaderStageKind) -> ShaderResult<Option<u32>> {
        match self.kind {
            PlannedDeclaration::Texture(texture) => texture.texture_binding(stage),
            PlannedDeclaration::Keep
            | PlannedDeclaration::MacroAliasedPosition(_)
            | PlannedDeclaration::Interface(_)
            | PlannedDeclaration::UniformMember(_) => Ok(None),
        }
    }

    /// Returns the source qualifier that should be removed while keeping the
    /// rest of this declaration.
    fn removed_qualifier(&self, source: &str) -> Option<SourceSpan> {
        let PlannedDeclaration::MacroAliasedPosition(declaration) = &self.kind else {
            return None;
        };
        let qualifier = declaration.qualifier.source_text();
        let declaration_source = source.get(self.span.start()..self.span.end())?;
        let relative_start = declaration_source.find(qualifier)?;
        SourceSpan::new(
            self.span.start() + relative_start,
            self.span.start() + relative_start + qualifier.len(),
        )
        .ok()
    }
}

impl TopLevelQualifier {
    /// Returns the source spelling for this top-level qualifier.
    const fn source_text(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Attribute => "attribute",
            Self::Varying => "varying",
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Legalization action selected for an original declaration.
enum PlannedDeclaration<'src> {
    /// Keep the original declaration in place.
    Keep,
    /// Keep a macro-aliased position declaration as a private variable.
    MacroAliasedPosition(MacroAliasedPositionDeclaration),
    /// Re-emit the declaration as a stage input or output.
    Interface(StageInterface<'src>),
    /// Re-emit the declaration as a separated texture handle.
    Texture(TextureDeclaration<'src>),
    /// Move the declaration into the generated global uniform block.
    UniformMember(UniformMember<'src>),
}

impl<'src> PlannedDeclaration<'src> {
    /// Builds a planned stage interface or private macro-aliased position.
    fn interface(
        module: &ShaderModule<'src>,
        declaration: &ShaderDeclaration<'src>,
        qualifier: TopLevelQualifier,
        direction: InterfaceDirection,
        ty: &'src str,
        name: &'src str,
    ) -> Self {
        if name == "_ww_sv_position" {
            return Self::MacroAliasedPosition(MacroAliasedPositionDeclaration { qualifier });
        }

        Self::Interface(StageInterface {
            direction,
            ty,
            name: Cow::Borrowed(name),
            array_suffix: ArraySuffix {
                module,
                declaration,
            }
            .source()
            .ok()
            .flatten(),
            location: None,
            local_copy: false,
            initializer: None,
        })
    }
}

/// Parsed declaration plus owning module used to plan legalization.
struct DeclarationSource<'module, 'src> {
    /// Parsed shader module containing the declaration.
    module: &'module ShaderModule<'src>,
    /// Source declaration being classified.
    declaration: &'module ShaderDeclaration<'src>,
}

impl<'src> TryFrom<DeclarationSource<'_, 'src>> for PlannedDeclaration<'src> {
    type Error = ShaderError;

    fn try_from(source: DeclarationSource<'_, 'src>) -> Result<Self, Self::Error> {
        let module = source.module;
        let declaration = source.declaration;
        Ok(
            match (
                declaration.qualifier(),
                declaration.type_name(),
                declaration.name(),
            ) {
                (Some(TopLevelQualifier::Uniform), Some(ty), Some(name)) => {
                    if let Some(sampler) = SamplerType::new(ty) {
                        if sampler.supports_texture_split() {
                            Self::Texture(TextureDeclaration {
                                ty: "texture2D",
                                name,
                                binding: None,
                                sampler_binding: None,
                            })
                        } else {
                            Self::Keep
                        }
                    } else {
                        Self::UniformMember(UniformMember {
                            ty,
                            name,
                            array_suffix: ArraySuffix {
                                module,
                                declaration,
                            }
                            .source()?
                            .map(Cow::Borrowed),
                            explicit_binding: ExplicitLayoutBinding::try_from(
                                DeclarationLayoutSource {
                                    module,
                                    declaration,
                                },
                            )
                            .ok()
                            .map(|binding| binding.binding),
                            binding: None,
                        })
                    }
                }
                (
                    Some(qualifier @ (TopLevelQualifier::Attribute | TopLevelQualifier::In)),
                    Some(ty),
                    Some(name),
                ) => Self::interface(
                    module,
                    declaration,
                    qualifier,
                    InterfaceDirection::Input,
                    ty,
                    name,
                ),
                (Some(qualifier @ TopLevelQualifier::Out), Some(ty), Some(name)) => {
                    Self::interface(
                        module,
                        declaration,
                        qualifier,
                        InterfaceDirection::Output,
                        ty,
                        name,
                    )
                }
                (Some(qualifier @ TopLevelQualifier::Varying), Some(ty), Some(name)) => {
                    let direction = match module.stage() {
                        ShaderStageKind::Vertex => InterfaceDirection::Output,
                        ShaderStageKind::Fragment => InterfaceDirection::Input,
                    };
                    Self::interface(module, declaration, qualifier, direction, ty, name)
                }
                _ => Self::Keep,
            },
        )
    }
}
