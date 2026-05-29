//! Resource layout declaration helpers.

use smol_str::SmolStr;

use super::resources::{TextureDeclaration, UniformMember};
use crate::{ShaderError, ShaderResult, ShaderStageKind, layout::DescriptorBinding};

/// Program-level descriptor resource layout edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StageResourceLayout {
    /// Descriptor binding assigned to the generated `GlobalUniforms` block.
    pub uniform_block_binding: Option<u32>,
    /// Program-level resource bindings kept in source by codegen.
    pub reserved_bindings: Vec<u32>,
    /// Program-level descriptor assignments for split texture declarations.
    texture_bindings: Vec<StageTextureResourceBinding>,
    /// Program-wide uniform members emitted by every stage.
    pub uniform_members: Vec<UniformMember>,
}

impl StageResourceLayout {
    /// Adds a program-level binding assignment for one split texture
    /// declaration.
    pub(crate) fn push_texture_binding(
        &mut self,
        stage: ShaderStageKind,
        name: SmolStr,
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

    /// Builds a resource allocator from program-level reservations.
    #[must_use]
    pub(crate) fn build_resource_layout_plan(&self) -> ResourceLayoutPlan {
        let mut plan = ResourceLayoutPlan::default();
        if let Some(binding) = self.uniform_block_binding {
            plan.reserve_available_binding(binding);
        }
        for binding in self.reserved_bindings.iter().copied() {
            plan.reserve_available_binding(binding);
        }
        for binding in self
            .texture_bindings
            .iter()
            .flat_map(|binding| [binding.texture_binding, binding.sampler_binding])
        {
            plan.reserve_available_binding(binding);
        }
        plan
    }

    /// Returns the program assignment for a split texture in this stage.
    #[must_use]
    pub(crate) fn binding_for_texture(
        &self,
        stage: ShaderStageKind,
        name: &str,
    ) -> Option<StageTextureResourceBinding> {
        self.texture_bindings
            .iter()
            .find(|binding| binding.matches(stage, name))
            .cloned()
    }
}

/// Program-level descriptor assignment for one stage split texture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StageTextureResourceBinding {
    /// Stage containing the source declaration.
    stage: ShaderStageKind,
    /// Source texture variable name.
    name: SmolStr,
    /// Descriptor binding assigned to the generated texture handle.
    pub texture_binding: u32,
    /// Descriptor binding assigned to the generated sampler.
    pub sampler_binding: u32,
}

impl StageTextureResourceBinding {
    /// Returns whether this assignment applies to a stage texture declaration.
    fn matches(&self, stage: ShaderStageKind, name: &str) -> bool {
        self.stage == stage && self.name == name
    }
}

/// Descriptor binding allocator that reserves texture-suffixed bindings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceLayoutPlan {
    /// Binding numbers already reserved or allocated.
    used: Vec<u32>,
    /// Encoded source texture bindings keyed by descriptor binding.
    encoded_textures: Vec<(u32, String)>,
}

impl ResourceLayoutPlan {
    /// Reserves one already assigned binding.
    pub(crate) fn reserve_binding(&mut self, binding: u32) -> ShaderResult<()> {
        if self.used.contains(&binding) {
            Err(ShaderError::invalid_request(
                "descriptor binding is already reserved",
            ))
        } else {
            self.used.push(binding);
            Ok(())
        }
    }

    /// Reserves bindings encoded by sampler names before allocating other
    /// resources.
    pub(crate) fn reserve_texture_bindings<'src>(
        &mut self,
        stage: ShaderStageKind,
        textures: impl Iterator<Item = TextureDeclaration<'src>>,
    ) -> ShaderResult<()> {
        for texture in textures {
            if let Some(binding) = texture.texture_binding(stage)? {
                if let Some((_, previous_name)) = self
                    .encoded_textures
                    .iter()
                    .find(|(reserved_binding, _)| *reserved_binding == binding)
                {
                    return Err(ShaderError::Codegen {
                        diagnostics: Box::new([texture.duplicate_binding_diagnostic(
                            stage,
                            previous_name,
                            binding,
                        )]),
                    });
                }
                self.encoded_textures
                    .push((binding, texture.name.to_owned()));
                self.reserve_available_binding(binding);
            }
        }
        Ok(())
    }

    /// Allocates the lowest unused descriptor binding in set zero.
    pub(crate) fn allocate(&mut self) -> ShaderResult<DescriptorBinding> {
        let mut binding = 0u32;
        while self.used.contains(&binding) {
            binding += 1;
        }
        self.used.push(binding);
        DescriptorBinding::new(0, binding)
    }

    /// Marks `binding` used when program-level planning already proved it is
    /// unique.
    fn reserve_available_binding(&mut self, binding: u32) {
        if !self.used.contains(&binding) {
            self.used.push(binding);
        }
    }
}
