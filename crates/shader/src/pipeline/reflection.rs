use std::collections::{BTreeMap, BTreeSet};

use crate::{ShaderError, ShaderReflection, ShaderResult, TextureSlot};

/// Merges stage reflection records with descriptor stage-mask unioning.
#[derive(Debug, Default)]
pub(super) struct ReflectionMerger {
    /// Descriptor bindings keyed by set/binding/kind/name.
    descriptors: BTreeMap<DescriptorKey, crate::ShaderDescriptorBinding>,
    /// Uniform blocks keyed by set/binding/name.
    uniform_blocks: BTreeMap<BlockKey, crate::ShaderUniformBlock>,
    /// Vertex inputs keyed by location.
    vertex_inputs: BTreeMap<u32, crate::ShaderVertexInput>,
    /// Active texture slots.
    active_texture_slots: BTreeSet<TextureSlot>,
}

impl ReflectionMerger {
    /// Appends one reflected stage payload.
    pub(super) fn push(&mut self, reflection: &ShaderReflection) -> ShaderResult<()> {
        for descriptor in reflection.descriptor_bindings() {
            let key = DescriptorKey::from(descriptor);
            if let Some(existing) = self.descriptors.get_mut(&key) {
                *existing = crate::ShaderDescriptorBinding::new(
                    existing.name().to_owned(),
                    existing.set(),
                    existing.binding(),
                    existing.kind(),
                    existing.stages().union(descriptor.stages()),
                    existing.count().max(descriptor.count()),
                )?;
            } else {
                let _old = self.descriptors.insert(key, descriptor.clone());
            }
        }

        for block in reflection.uniform_blocks() {
            let key = BlockKey::from(block);
            if let Some(existing) = self.uniform_blocks.get(&key) {
                if existing != block {
                    return Err(ShaderError::invalid_request(format!(
                        "conflicting reflected uniform block layout for `{}` at set {} binding {}",
                        block.name(),
                        block.set().set(),
                        block.binding().binding()
                    )));
                }
            } else {
                let _old = self.uniform_blocks.insert(key, block.clone());
            }
        }

        for input in reflection.vertex_inputs() {
            let _old = self
                .vertex_inputs
                .entry(input.location().index())
                .or_insert_with(|| input.clone());
        }

        self.active_texture_slots
            .extend(reflection.active_texture_slots().iter().copied());
        Ok(())
    }

    /// Builds merged reflection.
    pub(super) fn finish(self) -> ShaderReflection {
        ShaderReflection::new(
            self.descriptors
                .into_values()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            self.uniform_blocks
                .into_values()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            self.vertex_inputs
                .into_values()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            self.active_texture_slots
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }
}

/// Stable descriptor merge key.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DescriptorKey {
    /// Descriptor set.
    set: u32,
    /// Descriptor binding index.
    binding: u32,
    /// Descriptor kind label.
    kind: &'static str,
    /// Descriptor name.
    name: String,
}

impl From<&crate::ShaderDescriptorBinding> for DescriptorKey {
    fn from(binding: &crate::ShaderDescriptorBinding) -> Self {
        Self {
            set: binding.set().set(),
            binding: binding.binding().binding(),
            kind: match binding.kind() {
                crate::ShaderDescriptorKind::UniformBuffer => "uniform_buffer",
                crate::ShaderDescriptorKind::SampledImage => "sampled_image",
                crate::ShaderDescriptorKind::CombinedImageSampler => "combined_image_sampler",
                crate::ShaderDescriptorKind::Sampler => "sampler",
            },
            name: binding.name().to_owned(),
        }
    }
}

/// Stable uniform block merge key.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BlockKey {
    /// Descriptor set.
    set: u32,
    /// Descriptor binding index.
    binding: u32,
    /// Block name.
    name: String,
}

impl From<&crate::ShaderUniformBlock> for BlockKey {
    fn from(block: &crate::ShaderUniformBlock) -> Self {
        Self {
            set: block.set().set(),
            binding: block.binding().binding(),
            name: block.name().to_owned(),
        }
    }
}
