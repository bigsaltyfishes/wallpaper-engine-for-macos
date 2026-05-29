use std::num::NonZeroU32;

use shader::{
    ShaderDescriptorKind, ShaderError, ShaderReflector, ShaderStageKind, VertexFormat,
    reflect::NagaReflector,
};

fn parse_stage(stage: naga::ShaderStage, source: &str) -> naga::Module {
    let mut frontend = naga::front::glsl::Frontend::default();
    frontend
        .parse(&naga::front::glsl::Options::from(stage), source)
        .expect("test shader should parse")
}

#[test]
fn reflects_vertex_input_names_locations_and_formats() {
    let source = r#"
#version 450
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in uvec4 a_blend_indices;
layout(location = 3) in ivec2 a_signed_pair;

void main() {
    gl_Position = vec4(a_position, a_normal.z, 1.0) +
        vec4(a_blend_indices.x, a_signed_pair.x, 0, 0) * 0.0;
}
"#;

    let module = parse_stage(naga::ShaderStage::Vertex, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Vertex, &module)
        .expect("reflection should succeed");

    assert_eq!(reflection.vertex_inputs().len(), 4);
    assert_eq!(reflection.vertex_inputs()[0].name(), "a_position");
    assert_eq!(reflection.vertex_inputs()[0].location().index(), 0);
    assert_eq!(
        reflection.vertex_inputs()[0].format(),
        VertexFormat::R32G32Sfloat
    );
    assert_eq!(reflection.vertex_inputs()[1].name(), "a_normal");
    assert_eq!(reflection.vertex_inputs()[1].location().index(), 1);
    assert_eq!(
        reflection.vertex_inputs()[1].format(),
        VertexFormat::R32G32B32Sfloat
    );
    assert_eq!(reflection.vertex_inputs()[2].name(), "a_blend_indices");
    assert_eq!(reflection.vertex_inputs()[2].location().index(), 2);
    assert_eq!(
        reflection.vertex_inputs()[2].format(),
        VertexFormat::R32G32B32A32Uint
    );
    assert_eq!(reflection.vertex_inputs()[3].name(), "a_signed_pair");
    assert_eq!(reflection.vertex_inputs()[3].location().index(), 3);
    assert_eq!(
        reflection.vertex_inputs()[3].format(),
        VertexFormat::R32G32Sint
    );
}

#[test]
fn reflects_uniform_blocks_descriptors_and_member_offsets() {
    let source = r#"
#version 450
layout(location = 0) out vec4 fragColor;
layout(std140, set = 0, binding = 1) uniform GlobalUniforms {
    float g_Time;
    vec4 g_Tint;
};

void main() {
    fragColor = g_Tint * g_Time;
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");

    assert_eq!(reflection.uniform_blocks().len(), 1);
    let block = &reflection.uniform_blocks()[0];
    assert_eq!(block.name(), "GlobalUniforms");
    assert_eq!(block.set().set(), 0);
    assert_eq!(block.binding().binding(), 1);
    assert!(block.byte_size() >= 32);
    assert_eq!(block.members().len(), 2);
    assert_eq!(block.members()[0].name(), "g_Time");
    assert_eq!(block.members()[0].offset(), 0);
    assert_eq!(block.members()[1].name(), "g_Tint");
    assert_eq!(block.members()[1].offset(), 16);

    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "GlobalUniforms"
                && binding.set().set() == 0
                && binding.binding().binding() == 1
                && binding.kind() == ShaderDescriptorKind::UniformBuffer
                && binding.stages().fragment()
                && !binding.stages().vertex()
                && binding.count() == 1)
    );
}

#[test]
fn reflects_uniform_array_layout_metadata() {
    let source = r#"
#version 450
layout(location = 0) out vec4 fragColor;
layout(std140, set = 0, binding = 2) uniform ArrayUniforms {
    mat4 g_Mvp;
    vec4 g_Palette[3];
};

void main() {
    fragColor = g_Mvp[0] + g_Palette[2];
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");

    let block = &reflection.uniform_blocks()[0];
    assert_eq!(block.name(), "ArrayUniforms");
    assert_eq!(block.byte_size(), 112);

    let matrix = &block.members()[0];
    assert_eq!(matrix.name(), "g_Mvp");
    assert_eq!(matrix.byte_size(), 64);
    assert_eq!(matrix.element_count(), 16);
    assert_eq!(matrix.array_count(), 0);
    assert_eq!(matrix.array_stride(), 0);

    let palette = &block.members()[1];
    assert_eq!(palette.name(), "g_Palette");
    assert_eq!(palette.offset(), 64);
    assert_eq!(palette.byte_size(), 48);
    assert_eq!(palette.element_count(), 4);
    assert_eq!(palette.array_count(), 3);
    assert_eq!(palette.array_stride(), 16);
}

#[test]
fn reflects_only_live_uniform_blocks() {
    let source = r#"
#version 450
layout(location = 0) out vec4 fragColor;
layout(std140, set = 0, binding = 0) uniform LiveUniforms {
    vec4 g_Tint;
};
layout(std140, set = 0, binding = 1) uniform UnusedUniforms {
    vec4 g_UnusedTint;
};

void main() {
    fragColor = g_Tint;
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");
    let block_names: Vec<&str> = reflection
        .uniform_blocks()
        .iter()
        .map(shader::ShaderUniformBlock::name)
        .collect();
    let descriptor_names: Vec<&str> = reflection
        .descriptor_bindings()
        .iter()
        .map(shader::ShaderDescriptorBinding::name)
        .collect();

    assert_eq!(block_names, ["LiveUniforms"]);
    assert!(descriptor_names.contains(&"LiveUniforms"));
    assert!(!descriptor_names.contains(&"UnusedUniforms"));
}

#[test]
fn reflects_only_live_texture_descriptors() {
    let source = r#"
#version 450
layout(location = 0) in vec2 v_Uv;
layout(location = 0) out vec4 fragColor;
layout(set = 0, binding = 0) uniform texture2D g_Texture0;
layout(set = 0, binding = 3) uniform texture2D g_Texture3;
layout(set = 0, binding = 4) uniform texture2D g_Texture4;
layout(set = 0, binding = 5) uniform sampler g_Sampler;

void main() {
    fragColor =
        texture(sampler2D(g_Texture3, g_Sampler), v_Uv)
        + texture(sampler2D(g_Texture0, g_Sampler), v_Uv);
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");

    let slots: Vec<u8> = reflection
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();
    let descriptor_names: Vec<&str> = reflection
        .descriptor_bindings()
        .iter()
        .map(shader::ShaderDescriptorBinding::name)
        .collect();

    assert_eq!(slots, [0, 3]);
    assert!(descriptor_names.contains(&"g_Texture0"));
    assert!(descriptor_names.contains(&"g_Texture3"));
    assert!(descriptor_names.contains(&"g_Sampler"));
    assert!(!descriptor_names.contains(&"g_Texture4"));
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture3"
                && binding.set().set() == 0
                && binding.binding().binding() == 3
                && binding.kind() == ShaderDescriptorKind::SampledImage
                && binding.stages().fragment()
                && !binding.stages().vertex()
                && binding.count() == 1)
    );
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture0"
                && binding.set().set() == 0
                && binding.binding().binding() == 0
                && binding.kind() == ShaderDescriptorKind::SampledImage
                && binding.stages().fragment()
                && !binding.stages().vertex()
                && binding.count() == 1)
    );
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Sampler"
                && binding.set().set() == 0
                && binding.binding().binding() == 5
                && binding.kind() == ShaderDescriptorKind::Sampler
                && binding.stages().fragment()
                && !binding.stages().vertex()
                && binding.count() == 1)
    );
}

#[test]
fn active_texture_slots_follow_encoded_texture_names_not_descriptor_bindings() {
    let source = r#"
#version 450
layout(location = 0) in vec2 v_Uv;
layout(location = 0) out vec4 fragColor;
layout(set = 0, binding = 2) uniform texture2D u_Mask;
layout(set = 0, binding = 3) uniform sampler u_MaskSampler;
layout(set = 0, binding = 4) uniform texture2D g_Texture1;
layout(set = 0, binding = 5) uniform sampler g_TextureSampler;

void main() {
    fragColor =
        texture(sampler2D(u_Mask, u_MaskSampler), v_Uv)
        + texture(sampler2D(g_Texture1, g_TextureSampler), v_Uv);
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");
    let slots: Vec<u8> = reflection
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();

    assert_eq!(slots, [1]);
}

#[test]
fn reflects_split_texture_descriptors_with_per_texture_samplers() {
    let source = r#"
#version 450
layout(location = 0) in vec2 v_Uv;
layout(location = 0) out vec4 fragColor;
layout(set = 0, binding = 0) uniform texture2D g_Texture0;
layout(set = 0, binding = 1) uniform texture2D g_Texture1;
layout(set = 0, binding = 2) uniform sampler _we_Sampler_g_Texture0;
layout(set = 0, binding = 3) uniform sampler _we_Sampler_g_Texture1;

void main() {
    fragColor =
        texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv)
        + texture(sampler2D(g_Texture1, _we_Sampler_g_Texture1), v_Uv);
}
"#;

    let module = parse_stage(naga::ShaderStage::Fragment, source);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");

    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture0"
                && binding.kind() == ShaderDescriptorKind::SampledImage)
    );
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture1"
                && binding.kind() == ShaderDescriptorKind::SampledImage)
    );
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "_we_Sampler_g_Texture0"
                && binding.kind() == ShaderDescriptorKind::Sampler)
    );
    assert!(
        reflection
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "_we_Sampler_g_Texture1"
                && binding.kind() == ShaderDescriptorKind::Sampler)
    );
}

#[test]
fn reflects_texture_binding_arrays_with_counts() {
    let module = texture_binding_array_module();
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("reflection should succeed");

    let binding = reflection
        .descriptor_bindings()
        .iter()
        .find(|binding| binding.name() == "g_Textures")
        .expect("texture binding array should be reflected");

    assert_eq!(binding.binding().binding(), 6);
    assert_eq!(binding.kind(), ShaderDescriptorKind::SampledImage);
    assert_eq!(binding.count(), 2);
    assert!(binding.stages().fragment());
    assert!(!binding.stages().vertex());
    assert!(reflection.active_texture_slots().is_empty());
}

#[test]
fn reflects_active_texture_slots_by_g_texture_name_not_binding() {
    let module = sampled_texture_module("u_Albedo", 31);
    let reflection = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect("non-encoded texture should reflect successfully");

    let slots: Vec<u8> = reflection
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();

    assert!(slots.is_empty());
}

#[test]
fn rejects_leading_zero_encoded_active_texture_slot_as_reflection_error() {
    let module = sampled_texture_module("g_Texture01", 0);
    let error = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect_err("leading-zero encoded texture slot should be rejected");

    assert!(
        matches!(error, ShaderError::Reflection { ref message } if message.contains("canonical")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn rejects_encoded_active_texture_slot_above_model_range_as_reflection_error() {
    let module = sampled_texture_module("g_Texture32", 0);
    let error = NagaReflector
        .reflect_stage(ShaderStageKind::Fragment, &module)
        .expect_err("encoded slot 32 should exceed TextureSlot range");

    assert!(
        matches!(error, ShaderError::Reflection { ref message } if message.contains("model limits")),
        "unexpected error: {error:?}"
    );
}

fn sampled_texture_module(name: &str, binding: u32) -> naga::Module {
    let mut module = naga::Module::default();
    let texture_type = module.types.insert(
        naga::Type {
            name: Some("Texture2D".to_owned()),
            inner: naga::TypeInner::Image {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                class: naga::ImageClass::Sampled {
                    kind: naga::ScalarKind::Float,
                    multi: false,
                },
            },
        },
        naga::Span::default(),
    );
    let texture_global = module.global_variables.append(
        naga::GlobalVariable {
            name: Some(name.to_owned()),
            space: naga::AddressSpace::Handle,
            binding: Some(naga::ResourceBinding { group: 0, binding }),
            ty: texture_type,
            init: None,
            memory_decorations: naga::MemoryDecorations::empty(),
        },
        naga::Span::default(),
    );
    let mut function = naga::Function::default();
    let image = function.expressions.append(
        naga::Expression::GlobalVariable(texture_global),
        naga::Span::default(),
    );
    let old_length = function.expressions.len();
    let _query = function.expressions.append(
        naga::Expression::ImageQuery {
            image,
            query: naga::ImageQuery::Size { level: None },
        },
        naga::Span::default(),
    );
    function.body.push(
        naga::Statement::Emit(function.expressions.range_from(old_length)),
        naga::Span::default(),
    );
    function.body.push(
        naga::Statement::Return { value: None },
        naga::Span::default(),
    );
    module.entry_points.push(naga::EntryPoint {
        name: "main".to_owned(),
        stage: naga::ShaderStage::Fragment,
        early_depth_test: None,
        workgroup_size: [0; 3],
        workgroup_size_overrides: None,
        function,
        mesh_info: None,
        task_payload: None,
        incoming_ray_payload: None,
    });

    module
}

fn texture_binding_array_module() -> naga::Module {
    let mut module = naga::Module::default();
    let texture_type = module.types.insert(
        naga::Type {
            name: Some("Texture2D".to_owned()),
            inner: naga::TypeInner::Image {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                class: naga::ImageClass::Sampled {
                    kind: naga::ScalarKind::Float,
                    multi: false,
                },
            },
        },
        naga::Span::default(),
    );
    let texture_array_type = module.types.insert(
        naga::Type {
            name: Some("TextureArray".to_owned()),
            inner: naga::TypeInner::BindingArray {
                base: texture_type,
                size: naga::ArraySize::Constant(NonZeroU32::new(2).expect("nonzero count")),
            },
        },
        naga::Span::default(),
    );
    let texture_global = module.global_variables.append(
        naga::GlobalVariable {
            name: Some("g_Textures".to_owned()),
            space: naga::AddressSpace::Handle,
            binding: Some(naga::ResourceBinding {
                group: 0,
                binding: 6,
            }),
            ty: texture_array_type,
            init: None,
            memory_decorations: naga::MemoryDecorations::empty(),
        },
        naga::Span::default(),
    );
    let mut function = naga::Function::default();
    let base = function.expressions.append(
        naga::Expression::GlobalVariable(texture_global),
        naga::Span::default(),
    );
    let index = function.expressions.append(
        naga::Expression::Literal(naga::Literal::U32(1)),
        naga::Span::default(),
    );
    let old_length = function.expressions.len();
    let image = function.expressions.append(
        naga::Expression::Access { base, index },
        naga::Span::default(),
    );
    let _query = function.expressions.append(
        naga::Expression::ImageQuery {
            image,
            query: naga::ImageQuery::Size { level: None },
        },
        naga::Span::default(),
    );
    function.body.push(
        naga::Statement::Emit(function.expressions.range_from(old_length)),
        naga::Span::default(),
    );
    function.body.push(
        naga::Statement::Return { value: None },
        naga::Span::default(),
    );
    module.entry_points.push(naga::EntryPoint {
        name: "main".to_owned(),
        stage: naga::ShaderStage::Fragment,
        early_depth_test: None,
        workgroup_size: [0; 3],
        workgroup_size_overrides: None,
        function,
        mesh_info: None,
        task_payload: None,
        incoming_ray_payload: None,
    });

    module
}
