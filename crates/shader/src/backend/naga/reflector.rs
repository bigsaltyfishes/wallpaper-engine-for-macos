//! Naga-backed shader reflection.

use naga::valid::{Capabilities, ValidationFlags, Validator};

use crate::{
    BindingIndex, BindingSet, LocationIndex, ShaderDescriptorBinding, ShaderDescriptorKind,
    ShaderError, ShaderReflection, ShaderReflector, ShaderResult, ShaderStageKind, ShaderStageMask,
    ShaderUniformBlock, ShaderUniformMember, ShaderVertexInput, TextureSlot, VertexFormat,
};

/// Reflects renderer-neutral shader metadata from Naga IR.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NagaReflector;

impl ShaderReflector<naga::Module> for NagaReflector {
    fn reflect_stage(
        &self,
        stage: ShaderStageKind,
        module: &naga::Module,
    ) -> ShaderResult<ShaderReflection> {
        let info = Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(module)
            .map_err(|error| ShaderError::Reflection {
                message: error.to_string(),
            })?;

        ReflectionContext {
            stage,
            module,
            info: &info,
        }
        .reflect()
    }
}

/// Borrowed reflection state for one Naga module.
struct ReflectionContext<'module> {
    /// Stage requested by the caller.
    stage: ShaderStageKind,
    /// Validated Naga module being reflected.
    module: &'module naga::Module,
    /// Naga validation metadata for `module`.
    info: &'module naga::valid::ModuleInfo,
}

impl ReflectionContext<'_> {
    /// Reflects all supported renderer-neutral metadata.
    fn reflect(&self) -> ShaderResult<ShaderReflection> {
        Ok(ShaderReflection::new(
            self.descriptor_bindings()?,
            self.uniform_blocks()?,
            self.vertex_inputs()?,
            self.active_texture_slots()?,
        ))
    }

    /// Reflects descriptor bindings for global resources.
    fn descriptor_bindings(&self) -> ShaderResult<Box<[ShaderDescriptorBinding]>> {
        self.module
            .global_variables
            .iter()
            .filter_map(|(handle, global)| {
                self.is_global_used_by_stage(handle).then(|| {
                    ResourceGlobal::new(ShaderStageMask::single(self.stage), self.module, global)
                        .descriptor_binding()
                })?
            })
            .collect()
    }

    /// Reflects uniform-buffer struct layout.
    fn uniform_blocks(&self) -> ShaderResult<Box<[ShaderUniformBlock]>> {
        self.module
            .global_variables
            .iter()
            .filter(|(handle, global)| {
                global.space == naga::AddressSpace::Uniform && self.is_global_used_by_stage(*handle)
            })
            .map(|(_, global)| {
                UniformGlobal {
                    module: self.module,
                    global,
                }
                .into_block()
            })
            .collect()
    }

    /// Reflects vertex entry-point location inputs.
    fn vertex_inputs(&self) -> ShaderResult<Box<[ShaderVertexInput]>> {
        if self.stage != ShaderStageKind::Vertex {
            return Ok(Box::from([]));
        }

        let inputs = self
            .entry_points()
            .flat_map(|entry_point| entry_point.function.arguments.iter())
            .filter_map(|argument| {
                VertexArgument {
                    module: self.module,
                    argument,
                }
                .vertex_input()
            })
            .collect::<ShaderResult<Vec<_>>>()?;

        Ok(inputs.into_boxed_slice())
    }

    /// Reflects texture slots used by the requested entry point.
    fn active_texture_slots(&self) -> ShaderResult<Box<[TextureSlot]>> {
        let mut slots = Vec::new();

        for (handle, global) in self.module.global_variables.iter() {
            let resource = ResourceGlobal::new(ShaderStageMask::default(), self.module, global);
            if !self.is_global_used_by_stage(handle) || !resource.is_sampled_texture() {
                continue;
            }

            let Some(slot) = resource.texture_slot()? else {
                continue;
            };
            slots.push(
                TextureSlot::new(slot).map_err(|error| ShaderError::Reflection {
                    message: format!(
                        "reflected resource metadata is outside shader model limits: texture name \
                         cannot be represented as texture slot: {error}"
                    ),
                })?,
            );
        }

        slots.sort_unstable();
        slots.dedup();

        Ok(slots.into_boxed_slice())
    }

    /// Iterates entry points that match the requested shader stage.
    fn entry_points(&self) -> impl Iterator<Item = &naga::EntryPoint> {
        self.module
            .entry_points
            .iter()
            .filter(|entry_point| entry_point.stage == self.stage.into())
    }

    /// Returns whether a global is used by any matching entry point.
    fn is_global_used_by_stage(&self, handle: naga::Handle<naga::GlobalVariable>) -> bool {
        self.module
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry_point)| entry_point.stage == self.stage.into())
            .any(|(entry_point_index, _)| {
                !self.info.get_entry_point(entry_point_index)[handle].is_empty()
            })
    }
}

/// Naga global resource wrapper.
struct ResourceGlobal<'module> {
    /// Stages that use this resource global.
    stages: ShaderStageMask,
    /// Module that owns the global type.
    module: &'module naga::Module,
    /// Global variable to classify.
    global: &'module naga::GlobalVariable,
}

impl<'module> ResourceGlobal<'module> {
    /// Creates a resource global wrapper.
    const fn new(
        stages: ShaderStageMask,
        module: &'module naga::Module,
        global: &'module naga::GlobalVariable,
    ) -> Self {
        Self {
            stages,
            module,
            global,
        }
    }

    /// Returns a public descriptor binding when this global has one.
    fn descriptor_binding(&self) -> Option<ShaderResult<ShaderDescriptorBinding>> {
        let binding = self.global.binding?;
        let kind = self.kind()?;
        Some(
            BindingSet::new(binding.group)
                .and_then(|set| {
                    BindingIndex::new(binding.binding).and_then(|binding| {
                        ShaderDescriptorBinding::new(
                            self.name(),
                            set,
                            binding,
                            kind,
                            self.stages,
                            self.count(),
                        )
                    })
                })
                .map_err(|error| ShaderError::Reflection {
                    message: format!(
                        "reflected resource metadata is outside shader model limits: {error}"
                    ),
                }),
        )
    }

    /// Returns whether this global is a sampled texture resource.
    fn is_sampled_texture(&self) -> bool {
        matches!(
            self.resource_inner(),
            naga::TypeInner::Image {
                class: naga::ImageClass::Sampled { .. } | naga::ImageClass::Depth { .. },
                ..
            }
        )
    }

    /// Returns the Wallpaper Engine texture slot encoded in `g_TextureN`.
    fn texture_slot(&self) -> ShaderResult<Option<u8>> {
        let Some(suffix) = self.name().strip_prefix("g_Texture") else {
            return Ok(None);
        };
        if suffix.is_empty() || !suffix.chars().all(|character| character.is_ascii_digit()) {
            return Ok(None);
        }
        if suffix.len() > 1 && suffix.starts_with('0') {
            return Err(ShaderError::Reflection {
                message: format!(
                    "texture name `{}` is not a canonical g_TextureN slot name",
                    self.name()
                ),
            });
        }
        suffix
            .parse::<u8>()
            .map(Some)
            .map_err(|error| ShaderError::Reflection {
                message: format!("texture name slot suffix is invalid: {error}"),
            })
    }

    /// Returns the descriptor resource kind.
    fn kind(&self) -> Option<ShaderDescriptorKind> {
        match (self.global.space, self.resource_inner()) {
            (naga::AddressSpace::Uniform, _) => Some(ShaderDescriptorKind::UniformBuffer),
            (
                naga::AddressSpace::Handle,
                naga::TypeInner::Image {
                    class: naga::ImageClass::Sampled { .. } | naga::ImageClass::Depth { .. },
                    ..
                },
            ) => Some(ShaderDescriptorKind::SampledImage),
            (naga::AddressSpace::Handle, naga::TypeInner::Sampler { .. }) => {
                Some(ShaderDescriptorKind::Sampler)
            }
            _ => None,
        }
    }

    /// Returns the descriptor array count.
    fn count(&self) -> u32 {
        match self.type_inner() {
            naga::TypeInner::BindingArray { size, .. } => match size {
                naga::ArraySize::Constant(count) => count.get(),
                naga::ArraySize::Pending(_) | naga::ArraySize::Dynamic => 0,
            },
            _ => 1,
        }
    }

    /// Returns the global name or an empty fallback.
    fn name(&self) -> &'module str {
        self.global
            .name
            .as_deref()
            .or(self.module.types[self.global.ty].name.as_deref())
            .unwrap_or_default()
    }

    /// Returns the global variable type.
    fn type_inner(&self) -> &'module naga::TypeInner {
        &self.module.types[self.global.ty].inner
    }

    /// Returns the resource type, unwrapping descriptor arrays.
    fn resource_inner(&self) -> &'module naga::TypeInner {
        match self.type_inner() {
            naga::TypeInner::BindingArray { base, .. } => &self.module.types[*base].inner,
            inner => inner,
        }
    }
}

/// Uniform-buffer global wrapper.
struct UniformGlobal<'module> {
    /// Module that owns the global type.
    module: &'module naga::Module,
    /// Uniform global variable.
    global: &'module naga::GlobalVariable,
}

impl<'module> UniformGlobal<'module> {
    /// Converts this uniform global into a public uniform block.
    fn into_block(self) -> ShaderResult<ShaderUniformBlock> {
        let Some(binding) = self.global.binding else {
            return Err(ShaderError::Reflection {
                message: "uniform block is missing a resource binding".to_owned(),
            });
        };
        let naga::TypeInner::Struct { members, span } = &self.module.types[self.global.ty].inner
        else {
            return Err(ShaderError::Reflection {
                message: "uniform global does not point to a struct type".to_owned(),
            });
        };

        let members = members
            .iter()
            .map(|member| self.member_model(member))
            .collect::<ShaderResult<Vec<_>>>()?;

        ShaderUniformBlock::new(
            self.block_name(),
            BindingSet::new(binding.group)?,
            BindingIndex::new(binding.binding)?,
            *span,
            members.into_boxed_slice(),
        )
    }

    /// Converts one struct member into the public model.
    fn member_model(&self, member: &naga::StructMember) -> ShaderResult<ShaderUniformMember> {
        let layout = TypeLayout {
            module: self.module,
            ty: member.ty,
        };
        ShaderUniformMember::new(
            member.name.as_deref().unwrap_or_default(),
            member.offset,
            layout.byte_size(),
            layout.element_count(),
            layout.array_count(),
            layout.array_stride(),
        )
    }

    /// Returns the uniform block name or an empty fallback.
    fn block_name(&self) -> &'module str {
        self.global
            .name
            .as_deref()
            .or(self.module.types[self.global.ty].name.as_deref())
            .unwrap_or_default()
    }
}

/// Naga type layout helper.
struct TypeLayout<'module> {
    /// Module that owns the type.
    module: &'module naga::Module,
    /// Type handle to classify.
    ty: naga::Handle<naga::Type>,
}

impl TypeLayout<'_> {
    /// Returns the reflected byte size.
    fn byte_size(&self) -> u32 {
        self.type_inner().size(self.module.to_ctx())
    }

    /// Returns the scalar element count for vectors and matrices.
    fn element_count(&self) -> u32 {
        match self.type_inner() {
            naga::TypeInner::Scalar(_) => 1,
            naga::TypeInner::Vector { size, .. } => u32::from(*size),
            naga::TypeInner::Matrix { columns, rows, .. } => u32::from(*columns) * u32::from(*rows),
            naga::TypeInner::Array { base, .. } => TypeLayout {
                module: self.module,
                ty: *base,
            }
            .element_count(),
            _ => 0,
        }
    }

    /// Returns the constant array element count.
    fn array_count(&self) -> u32 {
        match self.type_inner() {
            naga::TypeInner::Array { size, .. } => match size {
                naga::ArraySize::Constant(count) => count.get(),
                naga::ArraySize::Pending(_) | naga::ArraySize::Dynamic => 0,
            },
            _ => 0,
        }
    }

    /// Returns the byte stride between array elements.
    fn array_stride(&self) -> u32 {
        match self.type_inner() {
            naga::TypeInner::Array { stride, .. } => *stride,
            _ => 0,
        }
    }

    /// Returns the type inner data.
    fn type_inner(&self) -> &'_ naga::TypeInner {
        &self.module.types[self.ty].inner
    }
}

/// Entry-point argument wrapper for vertex input reflection.
struct VertexArgument<'module> {
    /// Module that owns argument types.
    module: &'module naga::Module,
    /// Function argument to classify.
    argument: &'module naga::FunctionArgument,
}

impl VertexArgument<'_> {
    /// Returns a public vertex input when the argument has a location binding.
    fn vertex_input(&self) -> Option<ShaderResult<ShaderVertexInput>> {
        let Some(naga::Binding::Location { location, .. }) = self.argument.binding else {
            return None;
        };
        Some(LocationIndex::new(location).and_then(|location| {
            ShaderVertexInput::new(
                self.argument.name.as_deref().unwrap_or_default(),
                location,
                VertexArgumentFormat {
                    module: self.module,
                    ty: self.argument.ty,
                }
                .format()?,
            )
        }))
    }
}

/// Vertex format helper for Naga types.
struct VertexArgumentFormat<'module> {
    /// Module that owns the type.
    module: &'module naga::Module,
    /// Type handle to classify.
    ty: naga::Handle<naga::Type>,
}

impl VertexArgumentFormat<'_> {
    /// Returns the renderer-neutral vertex input format.
    fn format(&self) -> ShaderResult<VertexFormat> {
        match self.module.types[self.ty].inner {
            naga::TypeInner::Scalar(scalar) => Ok(VertexScalarFormat::try_from(scalar)?.format()),
            naga::TypeInner::Vector { size, scalar } => {
                Ok(VertexVectorFormat::try_from((size, scalar))?.format())
            }
            _ => Err(ShaderError::Reflection {
                message: "unsupported vertex input format".to_owned(),
            }),
        }
    }
}

/// Renderer-neutral format for one scalar vertex input.
struct VertexScalarFormat {
    /// Naga scalar kind for the input.
    kind: naga::ScalarKind,
}

impl TryFrom<naga::Scalar> for VertexScalarFormat {
    type Error = ShaderError;

    fn try_from(scalar: naga::Scalar) -> Result<Self, Self::Error> {
        if scalar.width == 4
            && matches!(
                scalar.kind,
                naga::ScalarKind::Float | naga::ScalarKind::Uint | naga::ScalarKind::Sint
            )
        {
            return Ok(Self { kind: scalar.kind });
        }

        Err(ShaderError::Reflection {
            message: "unsupported vertex input format".to_owned(),
        })
    }
}

impl VertexScalarFormat {
    /// Returns the renderer-neutral format for this scalar.
    const fn format(&self) -> VertexFormat {
        match self.kind {
            naga::ScalarKind::Float => VertexFormat::R32Sfloat,
            naga::ScalarKind::Uint => VertexFormat::R32Uint,
            naga::ScalarKind::Sint => VertexFormat::R32Sint,
            naga::ScalarKind::Bool
            | naga::ScalarKind::AbstractInt
            | naga::ScalarKind::AbstractFloat => unreachable!(),
        }
    }
}

/// Renderer-neutral format for one vector vertex input.
struct VertexVectorFormat {
    /// Naga vector width.
    size: naga::VectorSize,
    /// Naga scalar kind for the input.
    kind: naga::ScalarKind,
}

impl TryFrom<(naga::VectorSize, naga::Scalar)> for VertexVectorFormat {
    type Error = ShaderError;

    fn try_from((size, scalar): (naga::VectorSize, naga::Scalar)) -> Result<Self, Self::Error> {
        Ok(Self {
            size,
            kind: VertexScalarFormat::try_from(scalar)?.kind,
        })
    }
}

impl VertexVectorFormat {
    /// Returns the renderer-neutral format for this vector.
    const fn format(&self) -> VertexFormat {
        match (self.kind, self.size) {
            (naga::ScalarKind::Float, naga::VectorSize::Bi) => VertexFormat::R32G32Sfloat,
            (naga::ScalarKind::Float, naga::VectorSize::Tri) => VertexFormat::R32G32B32Sfloat,
            (naga::ScalarKind::Float, naga::VectorSize::Quad) => VertexFormat::R32G32B32A32Sfloat,
            (naga::ScalarKind::Uint, naga::VectorSize::Bi) => VertexFormat::R32G32Uint,
            (naga::ScalarKind::Uint, naga::VectorSize::Tri) => VertexFormat::R32G32B32Uint,
            (naga::ScalarKind::Uint, naga::VectorSize::Quad) => VertexFormat::R32G32B32A32Uint,
            (naga::ScalarKind::Sint, naga::VectorSize::Bi) => VertexFormat::R32G32Sint,
            (naga::ScalarKind::Sint, naga::VectorSize::Tri) => VertexFormat::R32G32B32Sint,
            (naga::ScalarKind::Sint, naga::VectorSize::Quad) => VertexFormat::R32G32B32A32Sint,
            (
                naga::ScalarKind::Bool
                | naga::ScalarKind::AbstractInt
                | naga::ScalarKind::AbstractFloat,
                _,
            ) => unreachable!(),
        }
    }
}

impl From<ShaderStageKind> for naga::ShaderStage {
    fn from(stage: ShaderStageKind) -> Self {
        match stage {
            ShaderStageKind::Vertex => Self::Vertex,
            ShaderStageKind::Fragment => Self::Fragment,
        }
    }
}
