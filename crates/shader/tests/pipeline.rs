use shader::{
    BindingIndex, BindingSet, ComboName, CompiledShaderStage, CompiledStageArtifact,
    InMemoryShaderSourceProvider, IncludePath, PropertyName, PropertyValue, ShaderCachePolicy,
    ShaderComboValue, ShaderCompiler, ShaderDescriptorKind, ShaderError, ShaderName,
    ShaderProgramRequest, ShaderReflection, ShaderReflector, ShaderResult, ShaderStageKind,
    ShaderStageSource, ShaderTextureInfo, ShaderUniformBlock, ShaderUniformMember,
    TextureFormatHint, TextureSlot,
    compile::NagaCompiler,
    legalize::LegalizedStageSource,
    pipeline::{DefaultShaderPipeline, ShaderPipeline, ShaderPipelineRevision},
};

const SPIRV_MAGIC: u32 = 0x0723_0203;

#[test]
fn compiles_program_and_merges_metadata_reflection_and_diagnostics() {
    let pipeline = pipeline();
    let request = request("1.0", "1");

    let program = pipeline
        .compile(&request)
        .expect("pipeline should compile both shader stages");

    assert_eq!(program.shader_name().as_str(), "effects/pipeline");
    assert_eq!(program.stages().len(), 2);
    assert!(
        program
            .stages()
            .iter()
            .all(|stage| stage.spirv().first() == Some(&SPIRV_MAGIC))
    );
    assert!(program.stages().iter().any(|stage| {
        stage.kind() == ShaderStageKind::Vertex
            && stage
                .legalized_source()
                .is_some_and(|source| source.contains("shared_uv"))
    }));

    assert_eq!(
        program.metadata().combos()[0].name().as_str(),
        "HAS_TEXTURE"
    );
    assert_eq!(program.metadata().combos()[0].value(), "1");
    assert_eq!(program.metadata().aliases()[0].material(), "brightness");
    assert_eq!(
        program.metadata().default_uniforms()[0].uniform(),
        "g_Brightness"
    );
    assert_eq!(program.metadata().default_textures()[0].slot().index(), 0);
    assert_eq!(program.metadata().active_texture_slots()[0].index(), 0);

    assert!(
        program
            .reflection()
            .vertex_inputs()
            .iter()
            .any(|input| input.name() == "a_Position" && input.location().index() == 0)
    );
    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture0"
                && binding.binding().binding() == 0
                && binding.stages().fragment())
    );
    assert!(
        program
            .reflection()
            .uniform_blocks()
            .iter()
            .any(|block| block.name() == "GlobalUniforms")
    );
    assert!(
        program
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.pass() == Some("Legalizer"))
    );
}

#[test]
fn cache_key_tracks_sources_combos_revision_and_legalized_output() {
    let pipeline = pipeline();
    let base = pipeline
        .compile(&request("1.0", "1"))
        .expect("base request should compile")
        .cache_key()
        .clone();
    let source_change = pipeline
        .compile(&request("2.0", "1"))
        .expect("source change should compile")
        .cache_key()
        .clone();
    let combo_change = pipeline
        .compile(&request("1.0", "0"))
        .expect("combo change should compile")
        .cache_key()
        .clone();
    let revision_change = pipeline
        .clone()
        .with_revision(ShaderPipelineRevision::new(4))
        .compile(&request("1.0", "1"))
        .expect("revision change should compile")
        .cache_key()
        .clone();
    let legalized_change = pipeline
        .compile(&request_with_fragment_call(
            "1.0",
            "texture(g_Texture0, v_Uv)",
            "1",
        ))
        .expect("legalized output change should compile")
        .cache_key()
        .clone();

    assert_ne!(base, source_change);
    assert_ne!(base, combo_change);
    assert_ne!(base, revision_change);
    assert_ne!(base, legalized_change);
}

#[test]
fn cache_key_compiler_options_identity_tracks_coordinate_space_adjustment_removal() {
    let pipeline = pipeline();
    let cache_key = pipeline
        .compile(&request("1.0", "1"))
        .expect("base request should compile")
        .cache_key()
        .clone();

    assert_eq!(cache_key.as_str(), "5c923dc72cd97d0e");
    assert_eq!(
        DefaultShaderPipeline::<InMemoryShaderSourceProvider>::compiler_options_cache_salt(),
        "naga-29.0.3-spv-no-coordinate-space-adjustment"
    );
}

#[test]
fn text_style_shader_preserves_uniform_block_binding_in_merged_reflection() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(ShaderName::new("text").expect("valid name"))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Vertex,
            concat!(
                "layout(binding = 1) uniform mat4 g_ModelViewProjectionMatrix;\n",
                "in vec3 a_Position;\n",
                "in vec2 a_TexCoord;\n",
                "out vec2 v_TexCoord;\n",
                "void main() {\n",
                "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
                "  v_TexCoord = a_TexCoord;\n",
                "}\n",
            ),
        ))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Fragment,
            concat!(
                "uniform sampler2D g_Texture0;\n",
                "in vec2 v_TexCoord;\n",
                "void main() {\n",
                "  gl_FragColor = texture(g_Texture0, v_TexCoord);\n",
                "}\n",
            ),
        ))
        .texture(ShaderTextureInfo::new(
            TextureSlot::new(0).expect("valid texture slot"),
            true,
            TextureFormatHint::Rgba8,
        ))
        .build()
        .expect("text-style request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("text-style request should compile");

    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| { binding.name() == "g_Texture0" && binding.binding().binding() == 0 })
    );
    assert!(
        program
            .reflection()
            .uniform_blocks()
            .iter()
            .any(|block| { block.name() == "GlobalUniforms" && block.binding().binding() == 1 })
    );
}

#[test]
fn generated_uniform_block_uses_one_program_binding_across_stages() {
    let pipeline = pipeline();
    let request =
        ShaderProgramRequest::builder(ShaderName::new("program_uniforms").expect("valid name"))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Vertex,
                concat!(
                    "uniform float g_Time;\n",
                    "in vec3 a_Position;\n",
                    "void main() {\n",
                    "  gl_Position = vec4(a_Position.xy * g_Time, a_Position.z, 1.0);\n",
                    "}\n",
                ),
            ))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Fragment,
                concat!(
                    "uniform sampler2D g_Texture0;\n",
                    "uniform sampler2D g_Texture1;\n",
                    "uniform float g_Opacity;\n",
                    "void main() {\n",
                    "  gl_FragColor = mix(\n",
                    "      texture(g_Texture0, vec2(0.25)),\n",
                    "      texture(g_Texture1, vec2(0.75)),\n",
                    "      g_Opacity);\n",
                    "}\n",
                ),
            ))
            .texture(ShaderTextureInfo::new(
                TextureSlot::new(0).expect("valid texture slot"),
                true,
                TextureFormatHint::Rgba8,
            ))
            .texture(ShaderTextureInfo::new(
                TextureSlot::new(1).expect("valid texture slot"),
                true,
                TextureFormatHint::Rgba8,
            ))
            .build()
            .expect("program-uniform request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program-uniform request should compile");
    let vertex_source = program
        .stages()
        .iter()
        .find(|stage| stage.kind() == ShaderStageKind::Vertex)
        .and_then(CompiledShaderStage::legalized_source)
        .expect("vertex legalized source should be captured");
    let fragment_source = program
        .stages()
        .iter()
        .find(|stage| stage.kind() == ShaderStageKind::Fragment)
        .and_then(CompiledShaderStage::legalized_source)
        .expect("fragment legalized source should be captured");
    let uniform_binding = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .map(|block| block.binding().binding())
        .expect("merged reflection should contain GlobalUniforms");
    let uniform_layout =
        format!("layout(std140, set = 0, binding = {uniform_binding}) uniform GlobalUniforms");

    assert!(vertex_source.contains(&uniform_layout));
    assert!(fragment_source.contains(&uniform_layout));
    assert_ne!(uniform_binding, 0);
    assert_ne!(uniform_binding, 1);

    assert_eq!(
        program
            .reflection()
            .uniform_blocks()
            .iter()
            .filter(|block| block.name() == "GlobalUniforms")
            .count(),
        1
    );
    let uniform_descriptors: Vec<_> = program
        .reflection()
        .descriptor_bindings()
        .iter()
        .filter(|binding| binding.name() == "GlobalUniforms")
        .collect();
    assert_eq!(uniform_descriptors.len(), 1);
    assert_eq!(uniform_descriptors[0].binding().binding(), uniform_binding);
    assert!(matches!(
        uniform_descriptors[0].kind(),
        ShaderDescriptorKind::UniformBuffer
    ));
    assert!(uniform_descriptors[0].stages().vertex());
    assert!(uniform_descriptors[0].stages().fragment());
}

#[test]
fn generated_uniform_block_uses_one_program_member_layout_across_stages() {
    let pipeline = pipeline();
    let vertex_source = concat!(
        "uniform mat4 g_ModelViewProjectionMatrix;\n",
        "in vec3 a_Position;\n",
        "void main() {\n",
        "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
        "}\n",
    );
    let fragment_source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform vec4 g_Texture0Resolution;\n",
        "uniform float g_Time;\n",
        "uniform float u_Opacity;\n",
        "void main() {\n",
        "  gl_FragColor = texture(g_Texture0, vec2(g_Time)) * u_Opacity + \
         vec4(g_Texture0Resolution.xy, 0.0, 0.0);\n",
        "}\n",
    );

    let program = pipeline
        .compile(&program_uniform_layout_request(
            "program_uniform_layout",
            [
                (ShaderStageKind::Vertex, vertex_source),
                (ShaderStageKind::Fragment, fragment_source),
            ],
        ))
        .expect("program-uniform-layout request should compile");
    let reversed_program = pipeline
        .compile(&program_uniform_layout_request(
            "program_uniform_layout_reversed",
            [
                (ShaderStageKind::Fragment, fragment_source),
                (ShaderStageKind::Vertex, vertex_source),
            ],
        ))
        .expect("reversed program-uniform-layout request should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    for member in [
        "mat4 g_ModelViewProjectionMatrix;",
        "vec4 g_Texture0Resolution;",
        "float g_Time;",
        "float u_Opacity;",
    ] {
        assert!(vertex_source.contains(member), "vertex missing {member}");
        assert!(
            fragment_source.contains(member),
            "fragment missing {member}"
        );
    }
    assert!(
        vertex_source.find("mat4 g_ModelViewProjectionMatrix;")
            < vertex_source.find("vec4 g_Texture0Resolution;")
    );
    assert_uniform_block_member_order(
        vertex_source,
        &[
            "mat4 g_ModelViewProjectionMatrix;",
            "vec4 g_Texture0Resolution;",
            "float g_Time;",
            "float u_Opacity;",
        ],
    );
    assert_uniform_block_member_order(
        fragment_source,
        &[
            "mat4 g_ModelViewProjectionMatrix;",
            "vec4 g_Texture0Resolution;",
            "float g_Time;",
            "float u_Opacity;",
        ],
    );

    let block = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reflection should contain GlobalUniforms");
    let member_names: Vec<_> = block.members().iter().map(|member| member.name()).collect();
    assert_eq!(
        member_names,
        [
            "g_ModelViewProjectionMatrix",
            "g_Texture0Resolution",
            "g_Time",
            "u_Opacity"
        ]
    );
    let member_offsets: Vec<_> = block
        .members()
        .iter()
        .map(|member| member.offset())
        .collect();
    assert_eq!(member_offsets, [0, 64, 80, 84]);

    let reversed_block = reversed_program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reversed reflection should contain GlobalUniforms");
    let reversed_member_names: Vec<_> = reversed_block
        .members()
        .iter()
        .map(|member| member.name())
        .collect();
    assert_eq!(reversed_member_names, member_names);
    let reversed_member_offsets: Vec<_> = reversed_block
        .members()
        .iter()
        .map(|member| member.offset())
        .collect();
    assert_eq!(reversed_member_offsets, member_offsets);
}

#[test]
fn generated_uniform_block_excludes_unsupported_sampler_uniforms() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_uniform_unsupported_samplers").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform mat4 g_ModelViewProjectionMatrix;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "layout(set = 0, binding = 0) uniform samplerCube g_Environment;\n",
            "layout(set = 0, binding = 2) uniform sampler g_Sampler;\n",
            "uniform float g_Exposure;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(g_Exposure);\n",
            "}\n",
        ),
    ))
    .build()
    .expect("program-uniform-sampler-cube request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program with unsupported sampler uniform should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);
    let uniform_block = fragment_source
        .lines()
        .find(|line| line.contains("uniform GlobalUniforms"))
        .expect("fragment should contain GlobalUniforms");

    assert!(
        fragment_source.contains("layout(set = 0, binding = 0) uniform samplerCube g_Environment;")
    );
    assert!(fragment_source.contains("layout(set = 0, binding = 2) uniform sampler g_Sampler;"));
    assert!(
        uniform_block.contains("binding = 1"),
        "GlobalUniforms should avoid the kept sampler binding: {uniform_block}"
    );
    assert_uniform_block_member_order(
        vertex_source,
        &["mat4 g_ModelViewProjectionMatrix;", "float g_Exposure;"],
    );
    assert_uniform_block_member_order(
        fragment_source,
        &["mat4 g_ModelViewProjectionMatrix;", "float g_Exposure;"],
    );
    assert!(!vertex_source.contains("samplerCube g_Environment;"));
    assert!(!vertex_source.contains("sampler g_Sampler;"));
    assert!(!uniform_block.contains("samplerCube g_Environment;"));
    assert!(!uniform_block.contains("sampler g_Sampler;"));
    assert_eq!(
        fragment_source
            .matches("samplerCube g_Environment;")
            .count(),
        1
    );
    assert_eq!(fragment_source.matches("sampler g_Sampler;").count(), 1);
}

#[test]
fn generated_texture_sampler_avoids_program_reserved_kept_sampler_binding() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_uniform_split_texture_reserved_sampler").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "layout(binding = 1) uniform samplerCube env;\n",
            "uniform sampler2D g_Texture0;\n",
            "uniform float g_Exposure;\n",
            "void main() {\n",
            "  gl_FragColor = texture(g_Texture0, vec2(0.5)) * g_Exposure;\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("program-uniform-reserved-sampler request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program with kept sampler and split texture should compile");
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);
    let uniform_block = fragment_source
        .lines()
        .find(|line| line.contains("uniform GlobalUniforms"))
        .expect("fragment should contain GlobalUniforms");

    assert!(fragment_source.contains("layout(binding = 1) uniform samplerCube env;"));
    assert!(
        fragment_source.contains("layout(std140, set = 0, binding = 2) uniform GlobalUniforms")
    );
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 3) uniform sampler _we_Sampler_g_Texture0;"),
        "generated sampler must avoid kept sampler binding:\n{fragment_source}"
    );
    assert!(!uniform_block.contains("samplerCube env;"));
    assert!(!uniform_block.contains("_we_Sampler_g_Texture0"));
    assert_uniform_block_member_order(fragment_source, &["float g_Exposure;"]);
}

#[test]
fn generated_texture_sampler_avoids_program_reserved_texture_binding_from_other_stage() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_uniform_split_texture_reserved_other_stage").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = texture(g_Texture0, vec2(0.5)) + vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D u_Mask;\n",
            "uniform float g_Exposure;\n",
            "void main() {\n",
            "  gl_FragColor = texture(u_Mask, vec2(0.5)) * g_Exposure;\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("program-uniform-cross-stage texture request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program with split texture in both stages should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(
        fragment_source.contains("layout(std140, set = 0, binding = 1) uniform GlobalUniforms")
    );
    assert!(
        vertex_source
            .contains("layout(set = 0, binding = 2) uniform sampler _we_Sampler_g_Texture0;"),
        "encoded texture sampler must be assigned before later stage resources:\n{vertex_source}"
    );
    assert!(fragment_source.contains("layout(set = 0, binding = 3) uniform texture2D u_Mask;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 4) uniform sampler _we_Sampler_u_Mask;"),
        "generated sampler must avoid other-stage texture binding 0 and \
         GlobalUniforms:\n{fragment_source}"
    );
    assert!(!fragment_source.contains("binding = 0) uniform sampler _we_Sampler_u_Mask;"));
    assert!(!fragment_source.contains("binding = 1) uniform sampler _we_Sampler_u_Mask;"));
    assert!(!fragment_source.contains("binding = 2) uniform sampler _we_Sampler_u_Mask;"));
}

#[test]
fn split_texture_rejects_same_name_kept_sampler_binding_collision() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_same_name_kept_sampler_collision").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "layout(binding = 0) uniform samplerCube g_Texture0;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = texture(g_Texture0, vec3(1.0, 0.0, 0.0)) + vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "void main() {\n",
            "  gl_FragColor = texture(g_Texture0, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("same-name collision request should be syntactically valid");

    let error = pipeline
        .compile(&request)
        .expect_err("incompatible same-name descriptors must not share binding 0");

    assert!(matches!(error, ShaderError::InvalidRequest { .. }));
}

#[test]
fn same_name_split_texture_uses_one_program_binding_across_stages() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_same_texture_both_stages").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  vec4 sampled = textureLod(g_Texture0, vec2(0.5), 0.0);\n",
            "  gl_Position = vec4(a_Position.xy + sampled.xy * 0.0, a_Position.z, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "void main() {\n",
            "  gl_FragColor = texture(g_Texture0, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("same split texture request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("same split texture in both stages should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(
        vertex_source
            .contains("layout(set = 0, binding = 1) uniform sampler _we_Sampler_g_Texture0;"),
        "{vertex_source}"
    );
    assert!(fragment_source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 1) uniform sampler _we_Sampler_g_Texture0;"),
        "{fragment_source}"
    );

    let texture_bindings: Vec<_> = program
        .reflection()
        .descriptor_bindings()
        .iter()
        .filter(|binding| binding.name() == "g_Texture0")
        .collect();
    assert_eq!(texture_bindings.len(), 1);
    assert_eq!(
        texture_bindings[0].kind(),
        ShaderDescriptorKind::SampledImage
    );
    assert_eq!(texture_bindings[0].binding().binding(), 0);
    assert!(texture_bindings[0].stages().vertex());
    assert!(texture_bindings[0].stages().fragment());

    let sampler_bindings: Vec<_> = program
        .reflection()
        .descriptor_bindings()
        .iter()
        .filter(|binding| binding.name() == "_we_Sampler_g_Texture0")
        .collect();
    assert_eq!(sampler_bindings.len(), 1);
    assert_eq!(sampler_bindings[0].kind(), ShaderDescriptorKind::Sampler);
    assert_eq!(sampler_bindings[0].binding().binding(), 1);
    assert!(sampler_bindings[0].stages().vertex());
    assert!(sampler_bindings[0].stages().fragment());
}

#[test]
fn kept_resource_in_nonzero_set_does_not_reserve_generated_set_zero_binding() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_set_aware_resource_reservations").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "layout(set = 1, binding = 0) uniform samplerCube env;\n",
            "uniform sampler2D u_Mask;\n",
            "void main() {\n",
            "  gl_FragColor = texture(u_Mask, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .build()
    .expect("nonzero-set kept resource request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("nonzero-set kept resource should not collide with set zero generated bindings");
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(fragment_source.contains("layout(set = 1, binding = 0) uniform samplerCube env;"));
    assert!(fragment_source.contains("layout(set = 0, binding = 0) uniform texture2D u_Mask;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 1) uniform sampler _we_Sampler_u_Mask;"),
        "{fragment_source}"
    );
}

#[test]
fn split_texture_descriptors_are_unique_across_program_stages() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_split_texture_unique_bindings").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform sampler2D u_VertexTex;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = texture(u_VertexTex, vec2(0.5)) + vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D u_Mask;\n",
            "uniform float g_Exposure;\n",
            "void main() {\n",
            "  gl_FragColor = texture(u_Mask, vec2(0.5)) * g_Exposure;\n",
            "}\n",
        ),
    ))
    .build()
    .expect("program split texture request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program with non-encoded split textures should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(set = 0, binding = 1) uniform texture2D u_VertexTex;"));
    assert!(
        vertex_source
            .contains("layout(set = 0, binding = 2) uniform sampler _we_Sampler_u_VertexTex;"),
        "{vertex_source}"
    );
    assert!(fragment_source.contains("layout(set = 0, binding = 3) uniform texture2D u_Mask;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 4) uniform sampler _we_Sampler_u_Mask;"),
        "{fragment_source}"
    );
}

#[test]
fn split_texture_descriptor_assignments_are_stable_when_stage_request_order_reverses() {
    let pipeline = source_capture_pipeline();
    let vertex_source = concat!(
        "uniform sampler2D u_VertexTex;\n",
        "in vec3 a_Position;\n",
        "void main() {\n",
        "  gl_Position = texture(u_VertexTex, vec2(0.5)) + vec4(a_Position, 1.0);\n",
        "}\n",
    );
    let fragment_source = concat!(
        "uniform sampler2D u_Mask;\n",
        "uniform float g_Exposure;\n",
        "void main() {\n",
        "  gl_FragColor = texture(u_Mask, vec2(0.5)) * g_Exposure;\n",
        "}\n",
    );

    let program = pipeline
        .compile(&program_split_texture_request(
            "program_split_texture_stable",
            [
                (ShaderStageKind::Vertex, vertex_source),
                (ShaderStageKind::Fragment, fragment_source),
            ],
        ))
        .expect("program with split textures should compile");
    let reversed_program = pipeline
        .compile(&program_split_texture_request(
            "program_split_texture_stable_reversed",
            [
                (ShaderStageKind::Fragment, fragment_source),
                (ShaderStageKind::Vertex, vertex_source),
            ],
        ))
        .expect("reversed program with split textures should compile");

    for compiled in [&program, &reversed_program] {
        let vertex_source = legalized_stage_source(compiled, ShaderStageKind::Vertex);
        let fragment_source = legalized_stage_source(compiled, ShaderStageKind::Fragment);

        assert!(
            vertex_source.contains("layout(set = 0, binding = 1) uniform texture2D u_VertexTex;")
        );
        assert!(
            vertex_source
                .contains("layout(set = 0, binding = 2) uniform sampler _we_Sampler_u_VertexTex;")
        );
        assert!(fragment_source.contains("layout(set = 0, binding = 3) uniform texture2D u_Mask;"));
        assert!(
            fragment_source
                .contains("layout(set = 0, binding = 4) uniform sampler _we_Sampler_u_Mask;")
        );
    }
}

#[test]
fn encoded_and_non_encoded_split_texture_descriptors_do_not_collide_across_stages() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_encoded_non_encoded_split_textures").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = texture(g_Texture0, vec2(0.5)) + vec4(a_Position, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D u_Mask;\n",
            "void main() {\n",
            "  gl_FragColor = texture(u_Mask, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("encoded/non-encoded split texture request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("program with encoded and non-encoded split textures should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(
        vertex_source
            .contains("layout(set = 0, binding = 1) uniform sampler _we_Sampler_g_Texture0;"),
        "{vertex_source}"
    );
    assert!(fragment_source.contains("layout(set = 0, binding = 2) uniform texture2D u_Mask;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 3) uniform sampler _we_Sampler_u_Mask;"),
        "{fragment_source}"
    );
}

#[test]
fn generated_uniform_block_rejects_duplicate_name_with_conflicting_type() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "uniform float g_Time;\n",
            "void main() {\n",
            "  gl_Position = vec4(g_Time);\n",
            "}\n",
        ),
        concat!(
            "uniform vec2 g_Time;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(g_Time, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let error = pipeline
        .compile(&request)
        .expect_err("conflicting duplicate uniform member type should be rejected");

    assert!(matches!(error, ShaderError::InvalidRequest { .. }));
    assert!(
        error
            .to_string()
            .contains("conflicting GlobalUniforms member declarations")
    );
}

#[test]
fn generated_uniform_block_rejects_duplicate_name_with_conflicting_array_suffix() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "uniform vec4 g_Offsets[2];\n",
            "void main() {\n",
            "  gl_Position = g_Offsets[0];\n",
            "}\n",
        ),
        concat!(
            "uniform vec4 g_Offsets[3];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[0];\n",
            "}\n",
        ),
    );

    let error = pipeline
        .compile(&request)
        .expect_err("conflicting duplicate uniform member array suffix should be rejected");

    assert!(matches!(error, ShaderError::InvalidRequest { .. }));
    assert!(
        error
            .to_string()
            .contains("conflicting GlobalUniforms member declarations")
    );
}

#[test]
fn generated_uniform_block_canonicalizes_numeric_array_suffixes() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "uniform vec4 g_Offsets[ 2 ];\n",
            "void main() {\n",
            "  gl_Position = g_Offsets[0];\n",
            "}\n",
        ),
        concat!(
            "uniform vec4 g_Offsets[02];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[1];\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("equivalent numeric array suffixes should share one generated member");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert_uniform_block_member_order(vertex_source, &["vec4 g_Offsets[2];"]);
    assert_uniform_block_member_order(fragment_source, &["vec4 g_Offsets[2];"]);
    assert!(!vertex_source.contains("[ 2 ]"));
    assert!(!fragment_source.contains("[02]"));
}

#[test]
fn generated_uniform_block_rejects_stage_local_struct_uniforms_before_codegen() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        "void main() { gl_Position = vec4(0.0); }\n",
        concat!(
            "struct Params { float value; };\n",
            "uniform Params u_Params;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(u_Params.value);\n",
            "}\n",
        ),
    );

    let error = pipeline
        .compile(&request)
        .expect_err("stage-local struct uniform should be rejected before codegen");

    assert!(matches!(error, ShaderError::InvalidRequest { .. }));
    assert!(
        error
            .to_string()
            .contains("unsupported GlobalUniforms member type")
    );
}

#[test]
fn generated_uniform_block_resolves_stage_local_leading_array_macro_before_codegen() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        "void main() { gl_Position = vec4(0.0); }\n",
        concat!(
            "#define LOCAL_COUNT 2\n",
            "uniform vec4 g_Offsets[LOCAL_COUNT];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[0];\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("stage-local leading uniform array macro should resolve before codegen");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(
        vertex_source.contains("vec4 g_Offsets[2];"),
        "vertex generated block should use resolved array size:\n{vertex_source}"
    );
    assert!(
        fragment_source.contains("vec4 g_Offsets[2];"),
        "fragment generated block should use resolved array size:\n{fragment_source}"
    );
    assert!(!vertex_source.contains("LOCAL_COUNT"));
}

#[test]
fn pipeline_rejects_conflicting_reflected_uniform_block_layouts() {
    let pipeline = ShaderPipeline::with_reflector(
        InMemoryShaderSourceProvider::new(),
        SourceCaptureCompiler,
        ConflictingUniformBlockReflector,
    );
    let request = interface_request(
        concat!(
            "uniform float g_Time;\n",
            "void main() {\n",
            "  gl_Position = vec4(g_Time);\n",
            "}\n",
        ),
        concat!(
            "uniform float g_Time;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(g_Time);\n",
            "}\n",
        ),
    );

    let error = pipeline
        .compile(&request)
        .expect_err("conflicting reflected GlobalUniforms layouts should be rejected");

    assert!(matches!(error, ShaderError::InvalidRequest { .. }));
    assert!(error.to_string().contains("GlobalUniforms"));
}

#[test]
fn generated_uniform_block_allows_matching_member_with_one_explicit_binding() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("program_uniform_explicit_binding").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "layout(binding = 2) uniform float g_Time;\n",
            "in vec3 a_Position;\n",
            "void main() {\n",
            "  gl_Position = vec4(a_Position.xy * g_Time, a_Position.z, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform float g_Time;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(g_Time);\n",
            "}\n",
        ),
    ))
    .build()
    .expect("program-uniform-explicit-binding request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("matching duplicate uniform member should compile");
    let block = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reflection should contain GlobalUniforms");

    assert_eq!(block.binding().binding(), 2);
    assert_eq!(
        block
            .members()
            .iter()
            .map(|member| member.name())
            .collect::<Vec<_>>(),
        ["g_Time"]
    );
}

#[test]
fn pipeline_preserves_layout_binding_and_array_suffix_facts_through_assembly() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "#define LOCAL_COUNT 4\n",
            "layout(binding = 5) uniform vec4 g_Offsets[LOCAL_COUNT];\n",
            "void main() {\n",
            "  gl_Position = g_Offsets[3];\n",
            "}\n",
        ),
        concat!(
            "#define LOCAL_COUNT 4\n",
            "uniform vec4 g_Offsets[LOCAL_COUNT];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[2];\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("layout binding and array suffix facts should assemble");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);
    let block = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reflection should contain GlobalUniforms");

    assert_eq!(block.binding().binding(), 5);
    assert_uniform_block_member_order(vertex_source, &["vec4 g_Offsets[4];"]);
    assert_uniform_block_member_order(fragment_source, &["vec4 g_Offsets[4];"]);
    assert!(vertex_source.contains("layout(std140, set = 0, binding = 5) uniform GlobalUniforms"));
    assert!(
        fragment_source.contains("layout(std140, set = 0, binding = 5) uniform GlobalUniforms")
    );
}

#[test]
fn pipeline_preserves_comment_separated_layout_binding_facts() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "layout /*outer*/ (binding /*inner*/ = 5) uniform vec4 g_Offsets[4];\n",
            "void main() {\n",
            "  gl_Position = g_Offsets[3];\n",
            "}\n",
        ),
        concat!(
            "uniform vec4 g_Offsets[4];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[2];\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("comment-separated layout binding should assemble");
    let block = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reflection should contain GlobalUniforms");

    assert_eq!(block.binding().binding(), 5);
}

#[test]
fn pipeline_preserves_first_binding_across_repeated_layout_qualifiers() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "layout(binding = 5) layout(std140) uniform vec4 g_Offsets[4];\n",
            "void main() {\n",
            "  gl_Position = g_Offsets[3];\n",
            "}\n",
        ),
        concat!(
            "uniform vec4 g_Offsets[4];\n",
            "void main() {\n",
            "  gl_FragColor = g_Offsets[2];\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("repeated layout qualifiers should preserve explicit binding");
    let block = program
        .reflection()
        .uniform_blocks()
        .iter()
        .find(|block| block.name() == "GlobalUniforms")
        .expect("merged reflection should contain GlobalUniforms");

    assert_eq!(block.binding().binding(), 5);
}

#[test]
fn active_texture_slots_follow_source_texture_names_not_descriptor_bindings() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/program_resource_slots").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform mat4 g_ModelViewProjectionMatrix;\n",
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "uniform sampler2D g_Texture1;\n",
            "uniform float g_Time;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = texture2D(g_Texture1, v_Uv) * g_Time;\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("valid request");

    let program = pipeline.compile(&request).expect("shader should compile");
    let active_slots: Vec<_> = program
        .reflection()
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();

    assert_eq!(active_slots, vec![1]);
}

#[test]
fn pipeline_rejects_leading_zero_encoded_source_texture_binding() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/leading_zero_texture_slot").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "attribute vec2 a_Position;\n",
            "void main() {\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture01;\n",
            "void main() {\n",
            "  gl_FragColor = texture2D(g_Texture01, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("valid request");

    let error = pipeline
        .compile(&request)
        .expect_err("leading-zero encoded texture slot should be rejected");

    let ShaderError::Legalize { diagnostics } = error else {
        panic!("expected structured legalization error");
    };
    let diagnostic = diagnostics
        .first()
        .expect("leading-zero rejection should include diagnostic");
    assert_eq!(diagnostic.pass(), Some("Legalizer"));
    assert!(diagnostic.message().contains("g_Texture01"));
    assert!(diagnostic.message().contains("canonical"));
}

#[test]
fn active_texture_slots_ignore_non_encoded_split_texture_descriptor_bindings() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/non_encoded_texture_slots").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "attribute vec2 a_Position;\n",
            "void main() {\n",
            "  vec4 sampled = textureLod(g_Texture0, vec2(0.5), 0.0);\n",
            "  gl_Position = sampled + vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D u_Mask;\n",
            "void main() {\n",
            "  gl_FragColor = texture2D(u_Mask, vec2(0.5));\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("valid request");

    let program = pipeline
        .compile(&request)
        .expect("shader with non-encoded split texture should compile");
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);
    let active_slots: Vec<_> = program
        .reflection()
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();
    let metadata_slots: Vec<_> = program
        .metadata()
        .active_texture_slots()
        .iter()
        .map(|slot| slot.index())
        .collect();

    assert!(fragment_source.contains("layout(set = 0, binding = 2) uniform texture2D u_Mask;"));
    assert!(
        fragment_source
            .contains("layout(set = 0, binding = 3) uniform sampler _we_Sampler_u_Mask;")
    );
    assert_eq!(active_slots, vec![0]);
    assert_eq!(metadata_slots, vec![0]);
}

#[test]
fn pipeline_extracts_metadata_from_expanded_includes_before_main() {
    let pipeline = ShaderPipeline::with_reflector(
        InMemoryShaderSourceProvider::new().with_source(
            IncludePath::new("common/material.glsl").expect("valid include path"),
            "uniform sampler2D g_Texture0; // {\"default\":\"white\",\"combo\":\"HAS_TEX\"}\n",
        ),
        SourceCaptureCompiler,
        EmptyReflector,
    );
    let request =
        ShaderProgramRequest::builder(ShaderName::new("include_meta").expect("valid name"))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Vertex,
                "void main() { gl_Position = vec4(0.0); }\n",
            ))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Fragment,
                concat!("#include \"common/material.glsl\"\n", "void main() {}\n",),
            ))
            .texture(ShaderTextureInfo::new(
                TextureSlot::new(0).expect("valid texture slot"),
                true,
                TextureFormatHint::Rgba8,
            ))
            .build()
            .expect("valid request");

    let program = pipeline
        .compile(&request)
        .expect("pipeline should compile include metadata source");

    assert_eq!(
        program.metadata().combos(),
        &[ShaderComboValue::new(
            ComboName::new("HAS_TEX").expect("valid combo"),
            "1"
        )]
    );
    assert_eq!(program.metadata().default_textures().len(), 1);
    assert_eq!(program.metadata().default_textures()[0].slot().index(), 0);
    assert_eq!(program.metadata().default_textures()[0].path(), "white");
}

#[test]
fn pipeline_extracts_inactive_combo_metadata_before_condition_stripping() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        "void main() { gl_Position = vec4(0.0); }\n",
        concat!(
            "#if 0\n",
            "// [COMBO] {\"combo\":\"HIDDEN\",\"default\":1}\n",
            "#endif\n",
            "void main() {}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("pipeline should compile inactive metadata source");

    assert_eq!(
        program.metadata().combos(),
        &[ShaderComboValue::new(
            ComboName::new("HIDDEN").expect("valid combo"),
            "1"
        )]
    );
}

#[test]
fn pipeline_uses_annotation_combo_defaults_as_compile_macros() {
    let pipeline = pipeline();
    let request = interface_request(
        "void main() { gl_Position = vec4(0.0); }\n",
        concat!(
            "// [COMBO] {\"combo\":\"BLENDMODE\",\"default\":30}\n",
            "// [COMBO] {\"combo\":\"RESOLUTION\",\"default\":32}\n",
            "// [COMBO] {\"combo\":\"INVERT\",\"default\":0}\n",
            "vec3 ApplyBlending(int mode, vec3 base, vec3 tint, float mask) {\n",
            "  return mode == 30 ? mix(base, tint, mask) : base;\n",
            "}\n",
            "void main() {\n",
            "  float frequency = 0.5 * float(RESOLUTION);\n",
            "  float mask = frequency + float(INVERT);\n",
            "  vec3 color = ApplyBlending(BLENDMODE, vec3(0.0), vec3(1.0), mask);\n",
            "  gl_FragColor = vec4(color, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("annotation combo defaults should be visible to live shader expressions");
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(fragment_source.contains("#define BLENDMODE 30"));
    assert!(fragment_source.contains("#define RESOLUTION 32"));
    assert!(fragment_source.contains("#define INVERT 0"));
}

#[test]
fn disabled_texture_slots_do_not_enable_texture_annotation_combos() {
    let pipeline = pipeline();
    let request =
        ShaderProgramRequest::builder(ShaderName::new("genericimage4").expect("valid name"))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Vertex,
                concat!(
                    "attribute vec2 a_Position;\n",
                    "void main() {\n",
                    "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
                    "}\n",
                ),
            ))
            .stage(ShaderStageSource::new(
                ShaderStageKind::Fragment,
                concat!(
                    "// [COMBO] {\"combo\":\"LIGHTING\",\"default\":1}\n",
                    "uniform sampler2D g_Texture0; // {\"default\":\"util/white\"}\n",
                    "#if LIGHTING\n",
                    "uniform sampler2D g_Texture1; // {\"combo\":\"NORMALMAP\",",
                    "\"format\":\"rg88\",\"formatcombo\":true}\n",
                    "#endif\n",
                    "void main() {\n",
                    "  vec4 color = texture2D(g_Texture0, vec2(0.5));\n",
                    "#if LIGHTING && NORMALMAP\n",
                    "  color += texture2D(g_Texture1, vec2(0.5));\n",
                    "#endif\n",
                    "  gl_FragColor = color;\n",
                    "}\n",
                ),
            ))
            .texture(ShaderTextureInfo::new(
                TextureSlot::new(0).expect("valid texture slot"),
                true,
                TextureFormatHint::Rgba8,
            ))
            .texture(ShaderTextureInfo::with_presence(
                TextureSlot::new(1).expect("valid texture slot"),
                false,
                false,
                TextureFormatHint::Unknown,
                [shader::TextureComponentState::disabled(); 3],
            ))
            .build()
            .expect("request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("disabled texture slot should compile");
    let normalmap = program
        .metadata()
        .combos()
        .iter()
        .find(|combo| combo.name().as_str() == "NORMALMAP")
        .expect("texture annotation should emit NORMALMAP");

    assert_eq!(normalmap.value(), "0");
    assert!(
        !program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| binding.name() == "g_Texture1"),
        "disabled normal-map texture should not be reflected as a live descriptor"
    );
}

#[test]
fn pipeline_resolves_combo_sized_uniform_arrays_in_generated_block() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("models/bonecount_array").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "uniform mat4x3 g_Bones[BONECOUNT];\n",
            "attribute vec3 a_Position;\n",
            "void main() {\n",
            "    vec3 localPos = a_Position;\n",
            "    localPos = mul(vec4(localPos, 1.0), g_Bones[0]);\n",
            "    gl_Position = vec4(localPos, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "void main() { gl_FragColor = vec4(1.0); }\n",
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("BONECOUNT").expect("valid combo"),
        "1",
    ))
    .build()
    .expect("request should be valid");

    let program = pipeline
        .compile(&request)
        .expect("combo-sized generated uniform array should compile");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);

    assert!(
        vertex_source.contains("mat4x3 g_Bones[1];"),
        "BONECOUNT should be resolved before Naga parses generated uniforms:\n{vertex_source}"
    );
    assert!(!vertex_source.contains("g_Bones[BONECOUNT]"));
}

#[test]
fn pipeline_does_not_restore_inactive_annotation_combo_default_after_request_combo_stabilizes() {
    let pipeline = source_capture_pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/default_texture_stabilized").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0; // {\"default\":\"fallback-r8.tex\"}\n",
            "#ifndef TEX0FORMAT\n",
            "// [COMBO] {\"combo\":\"FIRST_PASS_ONLY\",\"default\":1}\n",
            "#endif\n",
            "#if FIRST_PASS_ONLY\n",
            "uniform float g_StaleFirstPassMarker;\n",
            "#endif\n",
            "#if TEX0FORMAT == 9\n",
            "uniform sampler2D g_Texture1; // {\"default\":\"second-r8.tex\"}\n",
            "#endif\n",
            "#if TEX1FORMAT == 9\n",
            "uniform float g_CascadedDefaultObserved;\n",
            "#endif\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  vec4 color = texture(g_Texture0, v_Uv);\n",
            "#if TEX1FORMAT == 9\n",
            "  color += texture(g_Texture1, v_Uv);\n",
            "  color.r += g_CascadedDefaultObserved;\n",
            "#endif\n",
            "  gl_FragColor = color;\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::R8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::R8,
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("TEX0FORMAT").expect("valid combo"),
        "9",
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("TEX1FORMAT").expect("valid combo"),
        "9",
    ))
    .build()
    .expect("valid request");

    let program = pipeline
        .compile(&request)
        .expect("stabilized request should compile");
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(fragment_source.contains("g_CascadedDefaultObserved"));
    assert!(!fragment_source.contains("g_StaleFirstPassMarker"));
    assert!(!fragment_source.contains("#define FIRST_PASS_ONLY 1"));
}

#[test]
fn pipeline_reflects_split_images_and_per_texture_samplers() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/split_texture_samplers").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "uniform sampler2D g_Texture1;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = texture2D(g_Texture0, v_Uv) + texture2D(g_Texture1, v_Uv);\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("valid request");

    let program = pipeline.compile(&request).expect("shader should compile");

    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| {
                binding.name() == "g_Texture0"
                    && binding.kind() == ShaderDescriptorKind::SampledImage
            })
    );
    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| {
                binding.name() == "g_Texture1"
                    && binding.kind() == ShaderDescriptorKind::SampledImage
            })
    );
    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| {
                binding.name() == "_we_Sampler_g_Texture0"
                    && binding.kind() == ShaderDescriptorKind::Sampler
            })
    );
    assert!(
        program
            .reflection()
            .descriptor_bindings()
            .iter()
            .any(|binding| {
                binding.name() == "_we_Sampler_g_Texture1"
                    && binding.kind() == ShaderDescriptorKind::Sampler
            })
    );
}

#[test]
fn pipeline_omits_inactive_texture_descriptors_from_runtime_reflection() {
    let pipeline = pipeline();
    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/inactive_texture_descriptor").expect("valid name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        concat!(
            "uniform sampler2D g_Texture0;\n",
            "uniform sampler2D g_Texture1;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = texture2D(g_Texture0, v_Uv);\n",
            "}\n",
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("valid request");

    let program = pipeline.compile(&request).expect("shader should compile");
    let descriptor_names: Vec<&str> = program
        .reflection()
        .descriptor_bindings()
        .iter()
        .map(shader::ShaderDescriptorBinding::name)
        .collect();

    assert!(descriptor_names.contains(&"g_Texture0"));
    assert!(descriptor_names.contains(&"_we_Sampler_g_Texture0"));
    assert!(!descriptor_names.contains(&"g_Texture1"));
    assert!(!descriptor_names.contains(&"_we_Sampler_g_Texture1"));
}

#[test]
fn pipeline_preserves_inactive_metadata_without_resolving_inactive_missing_include() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        "void main() { gl_Position = vec4(0.0); }\n",
        concat!(
            "#if 0\n",
            "#include \"optional_or_platform_only.glsl\"\n",
            "// [COMBO] {\"combo\":\"HIDDEN\",\"default\":1}\n",
            "#endif\n",
            "void main() {}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("inactive missing include should not be resolved");

    assert_eq!(
        program.metadata().combos(),
        &[ShaderComboValue::new(
            ComboName::new("HIDDEN").expect("valid combo"),
            "1"
        )]
    );
}

#[test]
fn pipeline_synthesizes_vertex_output_for_fragment_only_varying() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "void main() {\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("fragment-only varying should be supplied by a synthesized vertex output");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_Uv;"));
    assert!(vertex_source.contains("v_Uv = vec2(0.0);"));
}

#[test]
fn pipeline_synthesized_vertex_output_initialization_compiles_through_naga() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec3 a_Position;\n",
            "void main() {\n",
            "  vec3 position = a_Position;\n",
            "  gl_Position = vec4(position, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_TexCoord;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("genericimage4-style synthesized vertex output should validate");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_TexCoord;"));
    assert!(vertex_source.contains("v_TexCoord = vec2(0.0);"));
}

#[test]
fn pipeline_synthesizes_unique_vertex_output_when_fragment_only_name_collides_with_vertex_input() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_Position = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("fragment-only varying should avoid colliding with vertex globals");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) in vec2 v_Uv;"));
    assert!(vertex_source.contains("layout(location = 0) out vec2 _we_out_v_Uv;"));
    assert!(vertex_source.contains("_we_out_v_Uv = vec2(0.0);"));
    assert!(!vertex_source.contains("layout(location = 0) out vec2 v_Uv;"));
    assert!(fragment_source.contains("layout(location = 0) in vec2 v_Uv;"));
}

#[test]
fn pipeline_synthesizes_unique_vertex_output_when_fragment_only_name_collides_with_vertex_global() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = vec2(1.0);\n",
            "  gl_Position = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("fragment-only varying should avoid colliding with vertex globals");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("vec2 v_Uv;"));
    assert!(vertex_source.contains("layout(location = 0) out vec2 _we_out_v_Uv;"));
    assert!(vertex_source.contains("_we_out_v_Uv = vec2(0.0);"));
    assert!(!vertex_source.contains("layout(location = 0) out vec2 v_Uv;"));
    assert!(fragment_source.contains("layout(location = 0) in vec2 v_Uv;"));
}

#[test]
fn pipeline_reserves_generated_vertex_output_names_for_later_fragment_only_varyings() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 v_Uv;\n",
            "attribute vec2 v_Uv_1;\n",
            "vec2 _we_out_v_Uv;\n",
            "void main() {\n",
            "  gl_Position = vec4(v_Uv + v_Uv_1 + _we_out_v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_Uv;\n",
            "varying vec2 v_Uv_1;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv + v_Uv_1, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("generated vertex outputs should be unique across fragment-only varyings");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);

    assert!(vertex_source.contains("layout(location = 0) out vec2 _we_out_v_Uv_1;"));
    assert!(vertex_source.contains("layout(location = 1) out vec2 _we_out_v_Uv_1_1;"));
    assert!(vertex_source.contains("_we_out_v_Uv_1 = vec2(0.0);"));
    assert!(vertex_source.contains("_we_out_v_Uv_1_1 = vec2(0.0);"));
}

#[test]
fn pipeline_keeps_vertex_only_varying_without_fragment_input() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!("void main() {\n", "  gl_FragColor = vec4(1.0);\n", "}\n",),
    );

    let program = pipeline
        .compile(&request)
        .expect("vertex-only varying should not block scene shader compilation");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_Uv;"));
    assert!(!fragment_source.contains("v_Uv"));
}

#[test]
fn pipeline_normalizes_cross_stage_varying_to_fragment_width_when_vertex_writes_subset() {
    let capture_pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec4 v_TexCoord;\n",
            "void main() {\n",
            "  v_TexCoord.xy = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_TexCoord;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = capture_pipeline
        .compile(&request)
        .expect("vertex vec4 varying written through .xy should match fragment vec2 input");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_TexCoord;"));
    assert!(vertex_source.contains("v_TexCoord.xy = a_Position;"));
    assert!(fragment_source.contains("layout(location = 0) in vec2 v_TexCoord;"));

    let _compiled = pipeline()
        .compile(&request)
        .expect("narrowed cross-stage varying should compile through Naga");
}

#[test]
fn pipeline_rejects_cross_stage_narrowing_when_vertex_writes_discarded_components() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec4 v_Uv;\n",
            "void main() {\n",
            "  v_Uv.xy = a_Position;\n",
            "  v_Uv.zw = vec2(1.0);\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let err = pipeline
        .compile(&request)
        .expect_err("discarded vertex output components should block narrowing");

    assert_cross_stage_error(
        err,
        "cross-stage varying `v_Uv` type mismatch: vertex outputs vec4 but fragment inputs vec2",
    );
}

#[test]
fn pipeline_does_not_export_macro_aliased_sv_position_as_cross_stage_output() {
    let capture_pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "#define gl_Position _ww_sv_position\n",
            "out vec2 v_TexCoord;\n",
            "out vec4 _ww_sv_position;\n",
            "void main() {\n",
            "  v_TexCoord = vec2(0.25, 0.75);\n",
            "  _ww_sv_position = vec4(0.0, 0.0, 0.0, 1.0);\n",
            "#undef gl_Position\n",
            "  gl_Position = _ww_sv_position;\n",
            "}\n",
        ),
        concat!(
            "in vec2 v_TexCoord;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = capture_pipeline
        .compile(&request)
        .expect("macro-aliased position should not participate in cross-stage IO");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_TexCoord;"));
    assert!(!vertex_source.contains("out vec4 _ww_sv_position"));
    assert!(!vertex_source.contains("layout(location = 1) out vec4 _ww_sv_position"));
    assert!(vertex_source.contains("vec4 _ww_sv_position;"));
    assert!(fragment_source.contains("layout(location = 0) in vec2 v_TexCoord;"));

    let _compiled = pipeline()
        .compile(&request)
        .expect("macro-aliased position source should compile through Naga");
}

#[test]
fn pipeline_narrows_plain_vertex_output_assignment_to_fragment_varying_width() {
    let capture_pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec3 a_Position;\n",
            "attribute vec2 a_TexCoord;\n",
            "uniform mat4 g_ModelViewProjectionMatrix;\n",
            "varying vec4 v_TexCoord;\n",
            "void main() {\n",
            "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
            "  v_TexCoord = a_TexCoord.xyxy;\n",
            "}\n",
        ),
        concat!(
            "varying vec2 v_TexCoord;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = capture_pipeline
        .compile(&request)
        .expect("vertex vec4 varying should normalize to fragment vec2 input");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_TexCoord;"));
    assert!(vertex_source.contains("v_TexCoord = a_TexCoord.xyxy.xy;"));

    let _compiled = pipeline()
        .compile(&request)
        .expect("narrowed vertex output assignment should compile through Naga");
}

#[test]
fn pipeline_compiles_vec4_varying_assigned_from_repeated_vec2_swizzle() {
    let request = interface_request(
        concat!(
            "attribute vec3 a_Position;\n",
            "attribute vec2 a_TexCoord;\n",
            "uniform mat4 g_ModelViewProjectionMatrix;\n",
            "varying vec4 v_TexCoord;\n",
            "varying vec2 v_TexCoordIris;\n",
            "void main() {\n",
            "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
            "  v_TexCoord = a_TexCoord.xyxy;\n",
            "  v_TexCoordIris = vec2(0.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec4 v_TexCoord;\n",
            "varying vec2 v_TexCoordIris;\n",
            "uniform sampler2D g_Texture0;\n",
            "void main() {\n",
            "  vec4 albedo = texture(g_Texture0, v_TexCoord.xy + v_TexCoordIris.xy);\n",
            "  gl_FragColor = albedo;\n",
            "}\n",
        ),
    );

    let _compiled = pipeline()
        .compile(&request)
        .expect("vec4 varying assigned from repeated vec2 swizzle should compile through Naga");
}

#[test]
fn pipeline_compiles_iris_movement_follow_cursor_vertex_path() {
    let request = interface_request(
        concat!(
            "// [COMBO] {\"material\":\"Follow \
             Cursor\",\"combo\":\"FOLLOWCURSOR\",\"default\":1}\n",
            "// [COMBO] {\"material\":\"Manual \
             Control\",\"combo\":\"MANUALCONTROL\",\"default\":0}\n",
            "uniform mat4 g_ModelViewProjectionMatrix;\n",
            "uniform mat4 g_EffectTextureProjectionMatrixInverse;\n",
            "uniform vec2 g_PointerPosition;\n",
            "uniform vec2 g_CursorScale;\n",
            "uniform vec2 g_CursorScaleMultiplier;\n",
            "uniform vec2 g_CursorScaleLimit;\n",
            "attribute vec3 a_Position;\n",
            "attribute vec2 a_TexCoord;\n",
            "varying vec4 v_TexCoord;\n",
            "varying vec2 v_TexCoordIris;\n",
            "void main() {\n",
            "  gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
            "  v_TexCoord = a_TexCoord.xyxy;\n",
            "#if FOLLOWCURSOR && !MANUALCONTROL\n",
            "  vec2 cursorPositionAdjusted = g_PointerPosition;\n",
            "  cursorPositionAdjusted.y = 1.0 - cursorPositionAdjusted.y;\n",
            "  cursorPositionAdjusted.x = (cursorPositionAdjusted.x - 0.5) * 2.0;\n",
            "  cursorPositionAdjusted.y = (cursorPositionAdjusted.y - 0.5) * 2.0;\n",
            "  vec4 transformedCursorPosition = g_EffectTextureProjectionMatrixInverse * \
             vec4(cursorPositionAdjusted, 0.0, 1.0);\n",
            "  transformedCursorPosition.xy = clamp(transformedCursorPosition.xy, \
             -g_CursorScaleLimit, g_CursorScaleLimit);\n",
            "  transformedCursorPosition.x *= -1.0;\n",
            "  vec2 da = transformedCursorPosition * g_CursorScale * g_CursorScaleMultiplier * \
             0.001;\n",
            "  v_TexCoordIris = da.xy;\n",
            "#endif\n",
            "}\n",
        ),
        concat!(
            "// [COMBO] {\"material\":\"ui_editor_properties_background\",\"combo\":\"BACKGROUND\",\"default\":0}\n",
            "varying vec4 v_TexCoord;\n",
            "varying vec2 v_TexCoordIris;\n",
            "uniform sampler2D g_Texture0;\n",
            "uniform sampler2D g_Texture1; // {\"combo\":\"MASK\"}\n",
            "void main() {\n",
            "  vec4 albedo = texture(g_Texture0, v_TexCoord.xy);\n",
            "#if MASK\n",
            "  albedo *= texture(g_Texture1, v_TexCoord.zw);\n",
            "#else\n",
            "  vec4 iris = texture(g_Texture0, v_TexCoord.xy + v_TexCoordIris.xy);\n",
            "#endif\n",
            "  gl_FragColor = iris;\n",
            "}\n",
        ),
    );

    let _compiled = pipeline()
        .compile(&request)
        .expect("iris movement follow-cursor vertex path should compile through Naga");
}

#[test]
fn pipeline_rejects_cross_stage_varying_type_mismatch_when_vertex_uses_missing_components() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec3 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 1.0);\n",
            "}\n",
        ),
    );

    let err = pipeline
        .compile(&request)
        .expect_err("cross-stage type mismatch should be rejected before compilation");

    assert_cross_stage_error(
        err,
        "cross-stage varying `v_Uv` type mismatch: vertex outputs vec2 but fragment inputs vec3",
    );
}

#[test]
fn pipeline_normalizes_fragment_input_to_vertex_width_when_fragment_reads_subset() {
    let capture_pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_AccumulationRate;\n",
            "void main() {\n",
            "  v_AccumulationRate.x = a_Position.x;\n",
            "  v_AccumulationRate.y = a_Position.y;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec3 v_AccumulationRate;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_AccumulationRate.x, v_AccumulationRate.y, 0.0, 1.0);\n",
            "}\n",
        ),
    );

    let program = capture_pipeline
        .compile(&request)
        .expect("fragment vec3 varying should normalize when only xy are read");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) out vec2 v_AccumulationRate;"));
    assert!(fragment_source.contains("layout(location = 0) in vec2 v_AccumulationRate;"));

    let _compiled = pipeline()
        .compile(&request)
        .expect("normalized fragment input should compile through Naga");
}

#[test]
fn workshop_3471294034_audio_buffer_accumulation_compiles_through_naga() {
    let pipeline = DefaultShaderPipeline::new(
        InMemoryShaderSourceProvider::new().with_source(
            IncludePath::new("common_blending.h").expect("valid include path"),
            "",
        ),
        NagaCompiler,
    );
    let request = ShaderProgramRequest::builder(
        ShaderName::new("workshop/3351849630/effects/audio_buffer_accumulation_accumulation")
            .expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        include_str!(
            "fixtures/3471294034/shaders/workshop/3351849630/effects/\
             audio_buffer_accumulation_accumulation.vert"
        ),
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        include_str!(
            "fixtures/3471294034/shaders/workshop/3351849630/effects/\
             audio_buffer_accumulation_accumulation.frag"
        ),
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(1).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .build()
    .expect("audio accumulation request should be valid");

    let _program = pipeline
        .compile(&request)
        .expect("audio accumulation shader should compile through Naga");
}

#[test]
fn pipeline_rejects_fragment_wider_varying_when_fragment_reads_missing_component() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_AccumulationRate;\n",
            "void main() {\n",
            "  v_AccumulationRate = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec3 v_AccumulationRate;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_AccumulationRate.z);\n",
            "}\n",
        ),
    );

    let err = pipeline
        .compile(&request)
        .expect_err("fragment reads outside vertex output width should be rejected");

    assert_cross_stage_error(
        err,
        "cross-stage varying `v_AccumulationRate` type mismatch: vertex outputs vec2 but fragment \
         inputs vec3",
    );
}

#[test]
fn pipeline_rejects_fragment_wider_varying_when_fragment_reads_whole_variable() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_AccumulationRate;\n",
            "void main() {\n",
            "  v_AccumulationRate = a_Position;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec3 v_AccumulationRate;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_AccumulationRate, 1.0);\n",
            "}\n",
        ),
    );

    let err = pipeline
        .compile(&request)
        .expect_err("whole-variable fragment read should require the fragment declaration width");

    assert_cross_stage_error(
        err,
        "cross-stage varying `v_AccumulationRate` type mismatch: vertex outputs vec2 but fragment \
         inputs vec3",
    );
}

#[test]
fn pipeline_treats_cross_stage_float1_varying_as_float() {
    let pipeline = pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying float1 v_Amount;\n",
            "void main() {\n",
            "  v_Amount = a_Position.x;\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying float v_Amount;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Amount);\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("float1 and float cross-stage varying types should normalize");

    assert!(program.stages().iter().any(|stage| {
        stage.kind() == ShaderStageKind::Vertex
            && stage
                .legalized_source()
                .is_some_and(|source| source.contains("layout(location = 0) out float v_Amount;"))
    }));
}

#[test]
fn pipeline_matches_cross_stage_varying_locations_by_name_not_declaration_order() {
    let pipeline = source_capture_pipeline();
    let request = interface_request(
        concat!(
            "attribute vec2 a_Position;\n",
            "varying vec2 v_Uv;\n",
            "varying vec4 v_Color;\n",
            "void main() {\n",
            "  v_Uv = a_Position;\n",
            "  v_Color = vec4(a_Position, 0.0, 1.0);\n",
            "  gl_Position = vec4(a_Position, 0.0, 1.0);\n",
            "}\n",
        ),
        concat!(
            "varying vec4 v_Color;\n",
            "varying vec2 v_Uv;\n",
            "void main() {\n",
            "  gl_FragColor = vec4(v_Uv, 0.0, 1.0) * v_Color;\n",
            "}\n",
        ),
    );

    let program = pipeline
        .compile(&request)
        .expect("same-name varyings should receive matching locations across stages");
    let vertex_source = legalized_stage_source(&program, ShaderStageKind::Vertex);
    let fragment_source = legalized_stage_source(&program, ShaderStageKind::Fragment);

    assert!(vertex_source.contains("layout(location = 0) out vec4 v_Color;"));
    assert!(fragment_source.contains("layout(location = 0) in vec4 v_Color;"));
    assert!(vertex_source.contains("layout(location = 1) out vec2 v_Uv;"));
    assert!(fragment_source.contains("layout(location = 1) in vec2 v_Uv;"));
}

fn pipeline() -> DefaultShaderPipeline<InMemoryShaderSourceProvider> {
    DefaultShaderPipeline::new(
        InMemoryShaderSourceProvider::new().with_source(
            IncludePath::new("common/shared.glsl").expect("valid include path"),
            "vec2 shared_uv(vec2 uv) { return uv; }\n",
        ),
        NagaCompiler,
    )
}

fn source_capture_pipeline()
-> ShaderPipeline<InMemoryShaderSourceProvider, SourceCaptureCompiler, EmptyReflector> {
    ShaderPipeline::with_reflector(
        InMemoryShaderSourceProvider::new(),
        SourceCaptureCompiler,
        EmptyReflector,
    )
}

#[derive(Clone, Debug)]
struct SourceCaptureCompiler;

impl ShaderCompiler for SourceCaptureCompiler {
    type Module = ();

    fn compile_stage(
        &self,
        stage: ShaderStageKind,
        source: &LegalizedStageSource,
    ) -> ShaderResult<CompiledStageArtifact<Self::Module>> {
        let compiled_stage = CompiledShaderStage::new(
            stage,
            Box::from([SPIRV_MAGIC]),
            Some(source.source().to_owned()),
            Box::from([]),
        );
        Ok(CompiledStageArtifact::new(
            compiled_stage,
            (),
            Box::from([]),
        ))
    }
}

#[derive(Clone, Debug)]
struct EmptyReflector;

impl ShaderReflector<()> for EmptyReflector {
    fn reflect_stage(
        &self,
        _stage: ShaderStageKind,
        _module: &(),
    ) -> ShaderResult<ShaderReflection> {
        Ok(ShaderReflection::empty())
    }
}

#[derive(Clone, Debug)]
struct ConflictingUniformBlockReflector;

impl ShaderReflector<()> for ConflictingUniformBlockReflector {
    fn reflect_stage(
        &self,
        stage: ShaderStageKind,
        _module: &(),
    ) -> ShaderResult<ShaderReflection> {
        let byte_size = match stage {
            ShaderStageKind::Vertex => 4,
            ShaderStageKind::Fragment => 8,
        };
        let member = ShaderUniformMember::new("g_Time", 0, byte_size, 1, 0, 0)?;
        let block = ShaderUniformBlock::new(
            "GlobalUniforms",
            BindingSet::new(0)?,
            BindingIndex::new(0)?,
            byte_size,
            Box::from([member]),
        )?;

        Ok(ShaderReflection::new(
            Box::from([]),
            Box::from([block]),
            Box::from([]),
            Box::from([]),
        ))
    }
}

fn legalized_stage_source(program: &shader::CompiledShaderProgram, stage: ShaderStageKind) -> &str {
    program
        .stages()
        .iter()
        .find(|compiled_stage| compiled_stage.kind() == stage)
        .and_then(CompiledShaderStage::legalized_source)
        .expect("stage should include legalized source")
}

fn assert_uniform_block_member_order(source: &str, members: &[&str]) {
    let block_start = source
        .find("uniform GlobalUniforms")
        .expect("source should contain GlobalUniforms");
    let block_end = source[block_start..]
        .find("};")
        .map(|offset| block_start + offset)
        .expect("GlobalUniforms block should close");
    let block = &source[block_start..block_end];
    let mut previous = 0usize;
    for member in members {
        let index = block
            .find(member)
            .unwrap_or_else(|| panic!("GlobalUniforms block missing {member}"));
        assert!(
            index >= previous,
            "GlobalUniforms member {member} was emitted out of order"
        );
        previous = index;
    }
}

fn request(brightness: &str, combo_value: &str) -> ShaderProgramRequest {
    request_with_fragment_call(brightness, "texture2D(g_Texture0, v_Uv)", combo_value)
}

fn request_with_fragment_call(
    brightness: &str,
    texture_call: &str,
    combo_value: &str,
) -> ShaderProgramRequest {
    ShaderProgramRequest::builder(ShaderName::new("effects/pipeline").expect("valid name"))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Vertex,
            concat!(
                "#include \"common/shared.glsl\"\n",
                "attribute vec2 a_Position;\n",
                "varying vec2 v_Uv;\n",
                "void main() {\n",
                "    v_Uv = shared_uv(a_Position);\n",
                "    gl_Position = vec4(a_Position, 0.0, 1.0);\n",
                "}\n",
            ),
        ))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Fragment,
            format!(
                concat!(
                    "varying vec2 v_Uv;\n",
                    "uniform sampler2D g_Texture0; // \
                     {{\"combo\":\"HAS_TEXTURE\",\"default\":\"materials/default.png\"}}\n",
                    "uniform float g_Brightness; // \
                     {{\"material\":\"brightness\",\"default\":{brightness}}}\n",
                    "void main() {{\n",
                    "    gl_FragColor = {texture_call} * g_Brightness;\n",
                    "}}\n",
                ),
                brightness = brightness,
                texture_call = texture_call,
            ),
        ))
        .texture(ShaderTextureInfo::new(
            TextureSlot::new(0).expect("valid texture slot"),
            true,
            TextureFormatHint::Rgba8,
        ))
        .property(shader::ProjectPropertyBinding::new(
            PropertyName::new("brightness").expect("valid property"),
            PropertyValue::Number(1.0),
        ))
        .cache_policy(ShaderCachePolicy::Enabled {
            scene_id: "pipeline-test".to_owned(),
        })
        .replace_combo(ShaderComboValue::new(
            ComboName::new("HAS_TEXTURE").expect("valid combo"),
            combo_value,
        ))
        .build()
        .expect("request should be valid")
}

fn program_uniform_layout_request(
    name: &str,
    stages: [(ShaderStageKind, &'static str); 2],
) -> ShaderProgramRequest {
    let mut builder = ShaderProgramRequest::builder(ShaderName::new(name).expect("valid name"));
    for (stage, source) in stages {
        builder = builder.stage(ShaderStageSource::new(stage, source));
    }
    builder
        .texture(ShaderTextureInfo::new(
            TextureSlot::new(0).expect("valid texture slot"),
            true,
            TextureFormatHint::Rgba8,
        ))
        .build()
        .expect("program-uniform-layout request should be valid")
}

fn program_split_texture_request(
    name: &str,
    stages: [(ShaderStageKind, &'static str); 2],
) -> ShaderProgramRequest {
    let mut builder = ShaderProgramRequest::builder(ShaderName::new(name).expect("valid name"));
    for (stage, source) in stages {
        builder = builder.stage(ShaderStageSource::new(stage, source));
    }
    builder
        .build()
        .expect("program split texture request should be valid")
}

fn interface_request(
    vertex: impl Into<String>,
    fragment: impl Into<String>,
) -> ShaderProgramRequest {
    ShaderProgramRequest::builder(ShaderName::new("effects/interface").expect("valid name"))
        .stage(ShaderStageSource::new(ShaderStageKind::Vertex, vertex))
        .stage(ShaderStageSource::new(ShaderStageKind::Fragment, fragment))
        .build()
        .expect("request should be valid")
}

fn assert_cross_stage_error(err: ShaderError, expected: &str) {
    let ShaderError::Legalize { diagnostics } = err else {
        panic!("expected cross-stage legalization diagnostic");
    };
    let diagnostic = diagnostics
        .first()
        .expect("cross-stage rejection should include diagnostic");

    assert_eq!(diagnostic.pass(), Some("PipelineInterface"));
    assert_eq!(diagnostic.message(), expected);
}
