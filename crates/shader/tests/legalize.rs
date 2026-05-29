use shader::{
    ShaderCompiler, ShaderError, ShaderStageKind,
    compile::NagaCompiler,
    legalize::{LegalizedStageSource, Legalizer},
    syntax::ShaderModule,
};

fn legalize(stage: ShaderStageKind, source: &str) -> LegalizedStageSource {
    let module = ShaderModule::parse(stage, source).expect("module parses");
    Legalizer.legalize(&module).expect("shader legalizes")
}

#[test]
fn legalizer_returns_stage_source_and_phase_diagnostics_without_mutating_module() {
    let source = "void main() { gl_FragColor = vec4(1.0); }\n";
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

    let legalized = Legalizer.legalize(&module).expect("shader legalizes");

    assert_eq!(legalized.stage(), ShaderStageKind::Fragment);
    assert!(legalized.source().contains("#version 450"));
    assert!(
        legalized
            .source()
            .contains("layout(location = 0) out vec4 _we_FragColor;")
    );
    assert!(
        legalized
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.pass() == Some("Legalizer"))
    );
}

#[test]
fn legalizes_vertex_attribute_and_varying_interface_layouts() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "attribute vec2 a_TexCoord;\n",
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    v_TexCoord = a_TexCoord;\n",
        "    gl_Position = vec4(a_Position, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(source.contains("#version 450"));
    assert!(source.contains("layout(location = 0) in vec3 a_Position;"));
    assert!(source.contains("layout(location = 1) in vec2 a_TexCoord;"));
    assert!(source.contains("layout(location = 0) out vec2 v_TexCoord;"));
    assert!(!source.contains("attribute "));
    assert!(!source.contains("varying vec2"));
}

#[test]
fn legalizes_fragment_varying_and_frag_color_output() {
    let source = concat!(
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(location = 0) in vec2 v_TexCoord;"));
    assert!(source.contains("layout(location = 0) out vec4 _we_FragColor;"));
    assert!(source.contains("_we_FragColor = vec4(v_TexCoord, 0.0, 1.0);"));
    assert!(!source.contains("gl_FragColor"));
}

#[test]
fn legalizes_hlsl_aliases_and_texture_sampling_helpers() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    float2 uv = float2(0.25, 0.75);\n",
        "    float3 mixed = lerp(float3(0.0), float3(1.0), saturate(0.5));\n",
        "    float4 color = tex2D(g_Texture0, uv) + texture2D(g_Texture0, uv);\n",
        "    gl_FragColor = float4(mixed, color.a);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 uv = vec2(0.25, 0.75);"));
    assert!(source.contains("vec3 mixed = mix(vec3(0.0), vec3(1.0), clamp(0.5, 0.0, 1.0));"));
    assert!(source.contains(
        "vec4 color = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) + \
         texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv);"
    ));
    assert!(source.contains("_we_FragColor = vec4(mixed, color.a);"));
    assert!(!source.contains("float2 uv"));
    assert!(!source.contains("float3 mixed"));
    assert!(!source.contains("float4 color"));
    assert!(!source.contains("tex2D(g_Texture0"));
    assert!(!source.contains("texture2D(g_Texture0"));
}

#[test]
fn legalizes_each_source_texture_with_its_own_sampler_descriptor() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform sampler2D g_Texture1;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    vec4 first = texture2D(g_Texture0, v_Uv);\n",
        "    vec4 second = texSample2D(g_Texture1, v_Uv);\n",
        "    gl_FragColor = first + second;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(source.contains("layout(set = 0, binding = 1) uniform texture2D g_Texture1;"));
    assert!(source.contains("uniform sampler _we_Sampler_g_Texture0;"));
    assert!(source.contains("uniform sampler _we_Sampler_g_Texture1;"));
    assert!(source.contains("texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv)"));
    assert!(source.contains("texture(sampler2D(g_Texture1, _we_Sampler_g_Texture1), v_Uv)"));
    assert!(!source.contains("_we_Sampler)"));
}

#[test]
fn keeps_unsupported_sampler_uniform_out_of_generated_uniform_block() {
    let source = concat!(
        "uniform samplerCube g_Environment;\n",
        "uniform float g_Exposure;\n",
        "void main() {\n",
        "    gl_FragColor = vec4(g_Exposure);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uniform samplerCube g_Environment;"));
    assert!(source.contains("layout(std140, set = 0, binding = 0) uniform GlobalUniforms"));
    assert!(source.contains("float g_Exposure;"));
    assert_eq!(source.matches("samplerCube g_Environment;").count(), 1);
}

#[test]
fn keeps_unknown_sampler_like_uniform_out_of_generated_uniform_block() {
    let source = concat!(
        "#extension GL_EXT_texture_shadow_lod : enable\n",
        "uniform sampler2DShadowEXT g_ShadowMap;\n",
        "uniform float g_Exposure;\n",
        "void main() {\n",
        "    gl_FragColor = vec4(g_Exposure);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();
    let uniform_block = source
        .lines()
        .find(|line| line.contains("uniform GlobalUniforms"))
        .expect("fragment should contain GlobalUniforms");

    assert!(source.contains("uniform sampler2DShadowEXT g_ShadowMap;"));
    assert!(source.contains("float g_Exposure;"));
    assert!(!uniform_block.contains("sampler2DShadowEXT g_ShadowMap;"));
    assert_eq!(source.matches("sampler2DShadowEXT g_ShadowMap;").count(), 1);
}

#[test]
fn workshop_shine_downsample2_renames_local_sample_keyword_before_naga() {
    let source = concat!(
        "void main() {\n",
        "    vec4 sample = vec4(0.25);\n",
        "    gl_FragColor = sample;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec4 sample_local = vec4(0.25);"),
        "effects/shine_downsample2 local `sample` should be renamed before Naga:\n{source}"
    );
    assert!(
        source.contains("_we_FragColor = sample_local;"),
        "effects/shine_downsample2 local `sample` use should be renamed before Naga:\n{source}"
    );
    assert!(!source.contains("vec4 sample ="));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("effects/shine_downsample2 local `sample` should compile after legalization");
}

#[test]
fn workshop_2798696916_macro_body_tex_sample_2d_is_legalized_before_naga() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "#define SharpenTap(uv) texSample2D(g_Texture0, uv)\n",
        "void main() {\n",
        "    gl_FragColor = SharpenTap(vec2(0.5));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "#define SharpenTap(uv) texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv)"
        ),
        "workshop/2798696916/effects/sharpen_filter macro body texSample2D should be \
         legalized:\n{source}"
    );
    assert!(!source.contains("texSample2D("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("workshop/2798696916/effects/sharpen_filter texSample2D macro should compile");
}

#[test]
fn legalizer_ignores_annotation_json_when_collecting_fixups() {
    let source = concat!(
        "#define INVERT 0\n",
        "// [COMBO] {\"material\":\"Invert mask\",\"combo\":\"INVERT\",\"type\":\"options\",",
        "\"default\":0}\n",
        "uniform sampler2D g_Texture0; // {\"material\":\"framebuffer\",",
        "\"label\":\"ui_editor_properties_framebuffer\",\"hidden\":true}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(
        source.contains("_we_FragColor = vec4(1.0);"),
        "annotation JSON must not be rewritten as shader code:\n{source}"
    );
}

#[test]
fn genericropeparticle_macro_body_cast3x3_is_legalized_before_naga() {
    let source = concat!(
        "uniform mat4 g_ModelMatrixInverse;\n",
        "#define RopeNormal(v) (CAST3X3(g_ModelMatrixInverse) * v)\n",
        "void main() {\n",
        "    vec3 normal = RopeNormal(vec3(0.0, 0.0, 1.0));\n",
        "    gl_FragColor = vec4(normal, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("#define RopeNormal(v) (mat3(g_ModelMatrixInverse) * v)"),
        "genericropeparticle macro body CAST3X3 should be legalized:\n{source}"
    );
    assert!(!source.contains("CAST3X3("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("genericropeparticle CAST3X3 macro should compile");
}

#[test]
fn genericropeparticle_header_cast3x3_define_and_use_are_legalized_before_naga() {
    let source = concat!(
        "#define CAST3X3(x) (mat3(x))\n",
        "uniform mat4 g_ModelMatrixInverse;\n",
        "attribute vec3 a_Position;\n",
        "void main() {\n",
        "    vec3 transformed = CAST3X3(g_ModelMatrixInverse) * a_Position;\n",
        "    gl_Position = vec4(transformed, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 transformed = mat3(g_ModelMatrixInverse) * a_Position;"),
        "header-style CAST3X3 use should be rewritten before Naga:\n{source}"
    );
    assert!(!source.contains("vec3 transformed = CAST3X3("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("genericropeparticle header CAST3X3 use should compile");
}

#[test]
fn genericropeparticle_nested_mul_rewrite_preserves_cast3x3_legalization_before_naga() {
    let source = concat!(
        "uniform mat4 g_ModelMatrixInverse;\n",
        "uniform vec3 g_OrientationForward;\n",
        "attribute vec3 a_Position;\n",
        "void main() {\n",
        "    vec3 eyeDirection = mul(g_OrientationForward, CAST3X3(g_ModelMatrixInverse));\n",
        "    gl_Position = vec4(eyeDirection + a_Position, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "vec3 eyeDirection = ((mat3(g_ModelMatrixInverse)) * (g_OrientationForward));"
        ),
        "nested mul rewrite should keep CAST3X3 legalized in copied arguments:\n{source}"
    );
    assert!(!source.contains("CAST3X3("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("genericropeparticle nested CAST3X3/mul expression should compile");
}

#[test]
fn hlsl_mul_rewrite_preserves_nested_texture_sampling_legalization() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform mat4 g_ColorTransform;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    vec4 color = mul(g_ColorTransform, texSample2D(g_Texture0, v_Uv));\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 color = ((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv)) * \
         (g_ColorTransform));"
    ));
    assert!(!source.contains("texSample2D("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested texture sampling inside HLSL mul should compile");
}

#[test]
fn hlsl_mul_rewrite_preserves_nested_reserved_identifier_legalization() {
    let source = concat!(
        "uniform mat4 g_ColorTransform;\n",
        "void main() {\n",
        "    vec4 sample = vec4(1.0);\n",
        "    vec4 color = mul(g_ColorTransform, sample);\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec4 sample_local = vec4(1.0);"));
    assert!(source.contains("vec4 color = ((sample_local) * (g_ColorTransform));"));
    assert!(!source.contains("vec4 color = ((sample) * (g_ColorTransform));"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested reserved identifier use inside HLSL mul should compile");
}

#[test]
fn hlsl_mul_rewrite_preserves_nested_type_coercion_legalization() {
    let source = concat!(
        "uniform mat4 g_ColorTransform;\n",
        "void main() {\n",
        "    vec4 color = mul(g_ColorTransform, max(0.25, vec4(1.0)));\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec4 color = ((max(vec4(0.25), vec4(1.0))) * (g_ColorTransform));"));
    assert!(!source.contains("max(0.25, vec4(1.0))"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested type coercion inside HLSL mul should compile");
}

#[test]
fn fmod_rewrite_preserves_nested_texture_sampling_legalization() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    float value = fmod(texSample2D(g_Texture0, v_Uv).x, 0.5);\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "float value = ((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv).x) - (0.5) \
         * trunc((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv).x) / (0.5)));"
    ));
    assert!(!source.contains("texSample2D("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested texture sampling inside fmod rewrite should compile");
}

#[test]
fn expression_replacements_preserve_deep_cross_policy_nesting() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform mat4 g_ColorTransform;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    bool sample = fmod(mul(g_ColorTransform, texSample2D(g_Texture0, v_Uv)).x, 0.5) > \
         0.25;\n",
        "    float value = 1.0;\n",
        "    value *= sample;\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("bool sample_local = "));
    assert!(source.contains(
        "bool sample_local = ((((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv)) * \
         (g_ColorTransform)).x) - (0.5) * trunc((((texture(sampler2D(g_Texture0, \
         _we_Sampler_g_Texture0), v_Uv)) * (g_ColorTransform)).x) / (0.5))) > 0.25;"
    ));
    assert!(
        source.contains("value *= (sample_local ? 1.0 : 0.0);"),
        "{source}"
    );
    assert!(!source.contains("value *= (sample ? 1.0 : 0.0);"));
    assert!(!source.contains("fmod("));
    assert!(!source.contains("mul("));
    assert!(!source.contains("texSample2D("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("deep nested fmod/mul/texture expression should compile");
}

#[test]
fn expression_replacements_preserve_more_than_two_nested_expression_fixups() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform mat4 g_ColorTransform;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    float value = log10(fmod(mul(g_ColorTransform, texSample2D(g_Texture0, v_Uv)).x, \
         0.5));\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "float value = (log2(((((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
             v_Uv)) * (g_ColorTransform)).x) - (0.5) * trunc((((texture(sampler2D(g_Texture0, \
             _we_Sampler_g_Texture0), v_Uv)) * (g_ColorTransform)).x) / (0.5)))) * \
             0.301029995663981);"
        ),
        "{source}"
    );
    assert!(!source.contains("log10("));
    assert!(!source.contains("fmod("));
    assert!(!source.contains("mul("));
    assert!(!source.contains("texSample2D("));
}

#[test]
fn policy_owned_texture_sampling_fixup_compiles_through_naga() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    vec4 color = texture2D(g_Texture0, v_Uv);\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source
            .contains("vec4 color = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_Uv);")
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("texture sampling policy-owned fixup should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn log10_rewrite_preserves_nested_cast_legalization() {
    let source = concat!(
        "uniform mat4 g_ModelMatrixInverse;\n",
        "void main() {\n",
        "    float value = log10(CAST3X3(g_ModelMatrixInverse)[0].x);\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source
            .contains("float value = (log2(mat3(g_ModelMatrixInverse)[0].x) * 0.301029995663981);")
    );
    assert!(!source.contains("CAST3X3("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested CAST3X3 inside log10 rewrite should compile");
}

#[test]
fn ddy_rewrite_preserves_nested_reserved_identifier_legalization() {
    let source = concat!(
        "void main() {\n",
        "    float sample = 0.5;\n",
        "    float derivative = ddy(sample);\n",
        "    gl_FragColor = vec4(derivative);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float sample_local = 0.5;"));
    assert!(source.contains("float derivative = dFdy(-(sample_local));"));
    assert!(!source.contains("float derivative = dFdy(-(sample));"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested reserved identifier use inside ddy rewrite should compile");
}

#[test]
fn genericimage4_vertex_skinning_uniform_array_dynamic_index_compiles_through_naga() {
    let source = concat!(
        "uniform mat4x3 g_Bones[1];\n",
        "attribute vec3 a_Position;\n",
        "attribute uvec4 a_BlendIndices;\n",
        "attribute vec4 a_BlendWeights;\n",
        "void main() {\n",
        "    vec3 localPos = a_Position;\n",
        "    localPos = mul(vec4(localPos, 1.0), g_Bones[a_BlendIndices.x] * a_BlendWeights.x);\n",
        "    gl_Position = vec4(localPos, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains("mat4x3 g_Bones[1];"),
        "genericimage4 skinning bone array should be preserved in generated uniforms:\n{source}"
    );
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("genericimage4 dynamic bone indexing should compile");
}

#[test]
fn genericimage4_vertex_skinning_weighted_bone_sum_compiles_through_naga() {
    let source = concat!(
        "uniform mat4x3 g_Bones[4];\n",
        "attribute vec3 a_Position;\n",
        "attribute uvec4 a_BlendIndices;\n",
        "attribute vec4 a_BlendWeights;\n",
        "void main() {\n",
        "    vec3 localPos = a_Position;\n",
        "    localPos = mul(vec4(localPos, 1.0), g_Bones[a_BlendIndices.x] * a_BlendWeights.x +\n",
        "                    g_Bones[a_BlendIndices.y] * a_BlendWeights.y +\n",
        "                    g_Bones[a_BlendIndices.z] * a_BlendWeights.z +\n",
        "                    g_Bones[a_BlendIndices.w] * a_BlendWeights.w);\n",
        "    gl_Position = vec4(localPos, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("genericimage4 weighted bone sum should compile");
}

#[test]
fn shake_vertex_audio_response_helper_compiles_through_naga() {
    let source = concat!(
        "uniform mat4 g_ModelViewProjectionMatrix;\n",
        "attribute vec3 a_Position;\n",
        "varying float v_AudioPulse;\n",
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform float g_AudioSpectrum16Right[16];\n",
        "uniform float g_AudioFrequencyMin;\n",
        "uniform float g_AudioFrequencyMax;\n",
        "uniform float g_AudioPower;\n",
        "uniform vec2 g_AudioBounds;\n",
        "uniform float g_AudioMultiply;\n",
        "float CreateAudioResponse(float bufferLeft[16], float bufferRight[16]) {\n",
        "    float audioResponse = 0.0;\n",
        "    for (int a = int(g_AudioFrequencyMin); a <= int(g_AudioFrequencyMax); ++a) {\n",
        "        audioResponse += bufferLeft[a];\n",
        "        audioResponse += bufferRight[a];\n",
        "    }\n",
        "    audioResponse /= (g_AudioFrequencyMax - g_AudioFrequencyMin + 1.0) * 2.0;\n",
        "    audioResponse = smoothstep(g_AudioBounds.x, g_AudioBounds.y, audioResponse);\n",
        "    audioResponse = saturate(pow(audioResponse, g_AudioPower)) * g_AudioMultiply;\n",
        "    return audioResponse;\n",
        "}\n",
        "void main() {\n",
        "    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);\n",
        "    v_AudioPulse = CreateAudioResponse(g_AudioSpectrum16Left, g_AudioSpectrum16Right);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains("float CreateAudioResponse()"),
        "shake audio helper should be specialized away from Naga-incompatible array \
         parameters:\n{source}"
    );
    assert!(
        source.contains("audioResponse += g_AudioSpectrum16Left[a];")
            && source.contains("audioResponse += g_AudioSpectrum16Right[a];"),
        "shake audio helper body should use the global arrays passed by every call:\n{source}"
    );
    assert!(
        source.contains("v_AudioPulse = CreateAudioResponse();"),
        "shake audio helper call should drop the specialized array arguments:\n{source}"
    );
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("shake audio response helper should compile");
}

#[test]
fn mixed_array_and_scalar_parameter_helper_compiles_through_naga() {
    let source = concat!(
        "uniform mat4 g_ModelViewProjectionMatrix;\n",
        "attribute vec3 a_Position;\n",
        "varying float v_Value;\n",
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform float g_Gain;\n",
        "float Helper(float samples[16], float gain) {\n",
        "    return samples[0] * gain;\n",
        "}\n",
        "void main() {\n",
        "    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);\n",
        "    v_Value = Helper(g_AudioSpectrum16Left, g_Gain);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains("float Helper(float gain)"),
        "mixed helper should preserve scalar parameters while specializing fixed arrays:\n{source}"
    );
    assert!(
        source.contains("return g_AudioSpectrum16Left[0] * gain;"),
        "mixed helper body should use the global array passed by every call:\n{source}"
    );
    assert!(
        source.contains("v_Value = Helper(g_Gain);"),
        "mixed helper call should preserve scalar arguments after dropping array \
         arguments:\n{source}"
    );
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("mixed array/scalar helper should compile");
}

#[test]
fn array_parameter_specialization_preserves_same_arity_scalar_overload() {
    let source = concat!(
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform float g_Value;\n",
        "float Helper(float samples[16]) {\n",
        "    return samples[0];\n",
        "}\n",
        "float Helper(float value) {\n",
        "    return value * 2.0;\n",
        "}\n",
        "void main() {\n",
        "    float arrayValue = Helper(g_AudioSpectrum16Left);\n",
        "    float scalarValue = Helper(g_Value);\n",
        "    gl_FragColor = vec4(arrayValue + scalarValue);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("float Helper()"),
        "array overload should be specialized:\n{source}"
    );
    assert!(
        source.contains("float Helper(float value)"),
        "scalar overload must stay callable with its scalar parameter:\n{source}"
    );
    assert!(
        source.contains("float arrayValue = Helper();"),
        "array overload call should drop only its array argument:\n{source}"
    );
    assert!(
        source.contains("float scalarValue = Helper(g_Value);"),
        "scalar overload call must not be rewritten:\n{source}"
    );
    assert!(
        source.contains("return g_AudioSpectrum16Left[0];"),
        "array helper body should use the matched global array:\n{source}"
    );
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("same-arity scalar overload should compile after array specialization");
}

#[test]
fn array_parameter_specialization_preserves_different_arity_overload() {
    let source = concat!(
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform float g_Value;\n",
        "float Helper(float samples[16]) {\n",
        "    return samples[1];\n",
        "}\n",
        "float Helper(float value, float scale) {\n",
        "    return value * scale;\n",
        "}\n",
        "void main() {\n",
        "    float arrayValue = Helper(g_AudioSpectrum16Left);\n",
        "    float scalarValue = Helper(g_Value, 3.0);\n",
        "    gl_FragColor = vec4(arrayValue + scalarValue);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("float Helper()"),
        "array overload should be specialized despite a same-name different-arity \
         overload:\n{source}"
    );
    assert!(
        source.contains("float Helper(float value, float scale)"),
        "different-arity overload must remain intact:\n{source}"
    );
    assert!(
        source.contains("float arrayValue = Helper();"),
        "array overload call should drop its array argument:\n{source}"
    );
    assert!(
        source.contains("float scalarValue = Helper(g_Value, 3.0);"),
        "different-arity overload call must not be rewritten:\n{source}"
    );
    assert!(
        source.contains("return g_AudioSpectrum16Left[1];"),
        "array helper body should use the matched global array:\n{source}"
    );
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("different-arity overload should compile after array specialization");
}

#[test]
fn array_parameter_specialization_rejects_different_top_level_array_call_sites() {
    let source = concat!(
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform float g_AudioSpectrum16Right[16];\n",
        "float Helper(float samples[16]) {\n",
        "    return samples[0];\n",
        "}\n",
        "void main() {\n",
        "    float left = Helper(g_AudioSpectrum16Left);\n",
        "    float right = Helper(g_AudioSpectrum16Right);\n",
        "    gl_FragColor = vec4(left + right);\n",
        "}\n",
    );
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

    let err = Legalizer
        .legalize(&module)
        .expect_err("ambiguous array helper specialization should be rejected");

    let ShaderError::Legalize { diagnostics } = err else {
        panic!("expected structured legalization error");
    };
    let diagnostic = diagnostics
        .first()
        .expect("array helper rejection should include diagnostic");
    assert_eq!(diagnostic.pass(), Some("Legalizer"));
    assert_eq!(
        diagnostic.message(),
        "array-parameter specialization requires each array parameter to use one stable top-level \
         array argument"
    );
}

#[test]
fn array_parameter_specialization_does_not_rewrite_member_fields() {
    let source = concat!(
        "struct SampleState {\n",
        "    float samples;\n",
        "};\n",
        "uniform float g_AudioSpectrum16Left[16];\n",
        "uniform SampleState g_State;\n",
        "float Helper(float samples[16], SampleState state) {\n",
        "    return samples[0] + state.samples;\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(Helper(g_AudioSpectrum16Left, g_State));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("return g_AudioSpectrum16Left[0] + state.samples;"),
        "array specialization must not rewrite struct fields named like removed \
         parameters:\n{source}"
    );
    assert!(
        source.contains("vec4(Helper(g_State))"),
        "array argument should still be removed from calls:\n{source}"
    );
    assert!(
        !source.contains("state.g_AudioSpectrum16Left"),
        "member field was incorrectly rewritten:\n{source}"
    );
}

#[test]
fn array_parameter_specialization_does_not_rewrite_shadowed_local_scopes() {
    let source = concat!(
        "uniform float g_AudioSpectrum16Left[16];\n",
        "float Helper(float samples[16]) {\n",
        "    float total = samples[0];\n",
        "    {\n",
        "        float samples = 2.0;\n",
        "        total += samples;\n",
        "    }\n",
        "    total += samples[1];\n",
        "    return total;\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(Helper(g_AudioSpectrum16Left));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("float total = g_AudioSpectrum16Left[0];"),
        "array parameter use before shadowing should be specialized:\n{source}"
    );
    assert!(
        source.contains("float samples = 2.0;") && source.contains("total += samples;"),
        "shadowed local declaration and uses should remain local:\n{source}"
    );
    assert!(
        source.contains("total += g_AudioSpectrum16Left[1];"),
        "array parameter use after shadowing scope should be specialized:\n{source}"
    );
    assert!(
        !source.contains("float g_AudioSpectrum16Left = 2.0;")
            && !source.contains("total += g_AudioSpectrum16Left;"),
        "shadowed local scope was incorrectly rewritten:\n{source}"
    );
}

#[test]
fn legalizes_top_level_float1_uniform_alias_inside_generated_block() {
    let source = concat!(
        "uniform float1 g_Amount;\n",
        "void main() {\n",
        "    gl_FragColor = vec4(g_Amount);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(std140, set = 0, binding = 0) uniform GlobalUniforms"));
    assert!(source.contains("    float g_Amount;"));
    assert!(!source.contains("float1 g_Amount"));
}

#[test]
fn texture_wrapper_preserves_nested_argument_legalization() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    float2 uv = float2(0.25, 0.75);\n",
        "    gl_FragColor = texture2D(g_Texture0, saturate(float2(uv.x, uv.y)));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "_we_FragColor = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), clamp(vec2(uv.x, \
         uv.y), 0.0, 1.0));"
    ));
    assert!(!source.contains("float2 uv"));
    assert!(!source.contains("saturate(float2"));
}

#[test]
fn copies_mutable_vertex_inputs_for_all_write_forms() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "attribute float a_Weight;\n",
        "void main() {\n",
        "    a_Position.xy = a_Position.xy * 2.0;\n",
        "    a_Position[0] += 1.0;\n",
        "    ++a_Weight;\n",
        "    a_Weight++;\n",
        "    gl_Position = vec4(a_Position * a_Weight, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(source.contains("layout(location = 0) in vec3 _we_in_a_Position;"));
    assert!(source.contains("layout(location = 1) in float _we_in_a_Weight;"));
    assert!(source.contains("vec3 a_Position = _we_in_a_Position;"));
    assert!(source.contains("float a_Weight = _we_in_a_Weight;"));
    assert!(source.contains("a_Position.xy = a_Position.xy * 2.0;"));
    assert!(source.contains("a_Position[0] += 1.0;"));
    assert!(source.contains("++a_Weight;"));
    assert!(source.contains("a_Weight++;"));
}

#[test]
fn read_only_vertex_input_arithmetic_does_not_create_local_copy() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "uniform float g_Bias;\n",
        "void main() {\n",
        "    float shifted = g_Bias + a_Position.x;\n",
        "    float scaled = a_Position.y * g_Bias;\n",
        "    gl_Position = vec4(a_Position.xy, shifted + scaled, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(source.contains("layout(location = 0) in vec3 a_Position;"));
    assert!(!source.contains("_we_in_a_Position"));
    assert!(!source.contains("vec3 a_Position = _we_in_a_Position;"));
}

#[test]
fn copies_mutable_fragment_inputs_but_preserves_read_only_inputs() {
    let source = concat!(
        "varying vec2 v_Uv;\n",
        "varying vec4 v_Color;\n",
        "void main() {\n",
        "    v_Uv.x += 0.25;\n",
        "    gl_FragColor = vec4(v_Uv, 0.0, 1.0) * v_Color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(location = 0) in vec2 _we_in_v_Uv;"));
    assert!(source.contains("layout(location = 1) in vec4 v_Color;"));
    assert!(source.contains("vec2 v_Uv = _we_in_v_Uv;"));
    assert!(source.contains("v_Uv.x += 0.25;"));
    assert!(source.contains("_we_FragColor = vec4(v_Uv, 0.0, 1.0) * v_Color;"));
    assert!(!source.contains("_we_in_v_Color"));
    assert!(!source.contains("vec4 v_Color = _we_in_v_Color;"));
}

#[test]
fn assigns_unique_resource_bindings_without_colliding_with_texture_zero() {
    let source = concat!(
        "uniform float g_Time;\n",
        "uniform sampler2D g_Texture0;\n",
        "uniform sampler2D g_Texture3;\n",
        "uniform sampler2D maskSampler;\n",
        "void main() {\n",
        "    gl_FragColor = texture2D(g_Texture3, vec2(g_Time)) + texture(maskSampler, \
         vec2(0.5));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("layout(set = 0, binding = 0) uniform texture2D g_Texture0;"));
    assert!(source.contains("layout(set = 0, binding = 3) uniform texture2D g_Texture3;"));
    assert!(source.contains("layout(std140, set = 0, binding = 1) uniform GlobalUniforms"));
    assert!(source.contains("layout(set = 0, binding = 2) uniform texture2D maskSampler;"));
    assert!(
        source.contains("layout(set = 0, binding = 4) uniform sampler _we_Sampler_g_Texture0;")
    );
    assert!(
        source.contains("layout(set = 0, binding = 5) uniform sampler _we_Sampler_g_Texture3;")
    );
    assert!(
        source.contains("layout(set = 0, binding = 6) uniform sampler _we_Sampler_maskSampler;")
    );
    assert!(source.contains(
        "_we_FragColor = texture(sampler2D(g_Texture3, _we_Sampler_g_Texture3), vec2(g_Time)) + \
         texture(sampler2D(maskSampler, _we_Sampler_maskSampler), vec2(0.5));"
    ));
    assert_eq!(source.matches("binding = 0").count(), 1);
    assert_eq!(source.matches("binding = 1").count(), 1);
    assert_eq!(source.matches("binding = 2").count(), 1);
    assert_eq!(source.matches("binding = 3").count(), 1);
    assert_eq!(source.matches("binding = 4").count(), 1);
    assert_eq!(source.matches("binding = 5").count(), 1);
    assert_eq!(source.matches("binding = 6").count(), 1);
}

#[test]
fn rejects_leading_zero_encoded_source_texture_binding() {
    let source = concat!(
        "uniform sampler2D g_Texture1;\n",
        "uniform sampler2D g_Texture01;\n",
        "void main() {\n",
        "    gl_FragColor = texture2D(g_Texture1, vec2(0.5)) + texture2D(g_Texture01, \
         vec2(0.5));\n",
        "}\n",
    );
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

    let err = Legalizer
        .legalize(&module)
        .expect_err("leading-zero encoded texture binding should be rejected");

    let ShaderError::Legalize { diagnostics } = err else {
        panic!("expected structured legalization error");
    };
    let diagnostic = diagnostics
        .first()
        .expect("duplicate texture binding should include diagnostic");
    assert_eq!(diagnostic.pass(), Some("Legalizer"));
    assert!(diagnostic.message().contains("g_Texture01"));
    assert!(diagnostic.message().contains("canonical"));
}

#[test]
fn preserves_explicit_uniform_binding_when_moving_uniform_into_block() {
    let source = concat!(
        "layout(binding = 1) uniform mat4 g_ModelViewProjectionMatrix;\n",
        "attribute vec3 a_Position;\n",
        "void main() {\n",
        "    gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(source.contains("layout(std140, set = 0, binding = 1) uniform GlobalUniforms"));
    assert!(source.contains("mat4 g_ModelViewProjectionMatrix;"));
    assert!(!source.contains("layout(std140, set = 0, binding = 0) uniform GlobalUniforms"));
}

#[test]
fn renames_user_defined_mod_without_guessing_from_argument_text() {
    let source = concat!(
        "float mod(float x) { return x; }\n",
        "void main() {\n",
        "    vec2 wrapped = mod(uv, period);\n",
        "    float scalar = mod(1.0);\n",
        "    gl_FragColor = vec4(wrapped, scalar, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x)"));
    assert!(source.contains("vec2 wrapped = mod(uv, period);"));
    assert!(source.contains("float scalar = _we_user_mod(1.0);"));
}

#[test]
fn renames_user_defined_two_arg_mod_declaration_and_call() {
    let source = concat!(
        "float mod(float value, float divisor) { return value - divisor; }\n",
        "void main() {\n",
        "    float wrapped = mod(5.5, 2.0);\n",
        "    gl_FragColor = vec4(wrapped);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float value, float divisor)"));
    assert!(source.contains("float wrapped = _we_user_mod(5.5, 2.0);"));
    assert!(!source.contains("float mod(float value, float divisor)"));
}

#[test]
fn renames_user_defined_two_arg_mod_calls_with_scalar_variables() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float wrapped = mod(x, y);\n",
        "    vec2 vector_wrapped = mod(vec2(x), vec2(y));\n",
        "    gl_FragColor = vec4(vector_wrapped, wrapped, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("float wrapped = _we_user_mod(x, y);"));
    assert!(source.contains("vec2 vector_wrapped = mod(vec2(x), vec2(y));"));
}

#[test]
fn renames_user_defined_two_arg_mod_calls_with_comma_scalar_variables() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5, y = 2.0;\n",
        "    float wrapped = mod(x, y);\n",
        "    gl_FragColor = vec4(wrapped);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("float wrapped = _we_user_mod(x, y);"));
    assert!(!source.contains("float wrapped = mod(x, y);"));
}

#[test]
fn user_mod_classification_uses_nearest_scalar_or_vector_binding() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float scalar = mod(x, y);\n",
        "    {\n",
        "        vec2 x = vec2(5.5);\n",
        "        vec2 y = vec2(2.0);\n",
        "        vec2 vector = mod(x, y);\n",
        "        gl_FragColor = vec4(vector, scalar, 1.0);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("float scalar = _we_user_mod(x, y);"));
    assert!(source.contains("vec2 vector = mod(x_local, y_local);"));
    assert!(!source.contains("vec2 vector = _we_user_mod(x, y);"));
    assert!(!source.contains("vec2 vector = _we_user_mod(x_local, y_local);"));
}

#[test]
fn user_mod_classification_ignores_function_prototype_parameters() {
    let source = concat!(
        "vec2 x;\n",
        "vec2 y;\n",
        "vec2 z;\n",
        "float mod(float x, float y) { return x - y; }\n",
        "void helper(float x, float y);\n",
        "void other(float y, float z) { }\n",
        "void main() {\n",
        "    vec2 wrapped = mod(x, y);\n",
        "    vec2 header_wrapped = mod(y, z);\n",
        "    gl_FragColor = vec4(wrapped + header_wrapped, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("vec2 wrapped = mod(x, y);"));
    assert!(source.contains("vec2 header_wrapped = mod(y, z);"));
    assert!(!source.contains("vec2 wrapped = _we_user_mod(x, y);"));
    assert!(!source.contains("vec2 header_wrapped = _we_user_mod(y, z);"));
}

#[test]
fn user_mod_classification_tracks_function_body_parameters() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "float helper(float x, float y) { return mod(x, y); }\n",
        "void main() {\n",
        "    gl_FragColor = vec4(helper(5.5, 2.0));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("return _we_user_mod(x, y);"));
}

#[test]
fn user_mod_classification_skips_parameter_qualifiers() {
    let source = concat!(
        "float mod(const float x, const float y) { return x - y; }\n",
        "void main() {\n",
        "    float wrapped = mod(5.5, 2.0);\n",
        "    gl_FragColor = vec4(wrapped);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(const float x, const float y)"));
    assert!(source.contains("float wrapped = _we_user_mod(5.5, 2.0);"));
}

#[test]
fn user_mod_classification_keeps_float_alias_vector_builtin_calls() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float2 x = float2(5.5);\n",
        "    float2 y = float2(2.0);\n",
        "    float2 wrapped = mod(x, y);\n",
        "    gl_FragColor = float4(wrapped, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("vec2 wrapped = mod(x, y);"));
    assert!(!source.contains("vec2 wrapped = _we_user_mod(x, y);"));
}

#[test]
fn user_mod_classification_keeps_integer_vector_shadowed_builtin_calls() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float scalar = mod(x, y);\n",
        "    {\n",
        "        ivec2 x = ivec2(5);\n",
        "        ivec2 y = ivec2(2);\n",
        "        ivec2 wrapped = mod(x, y);\n",
        "        gl_FragColor = vec4(vec2(wrapped), scalar, 1.0);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float scalar = _we_user_mod(x, y);"));
    assert!(source.contains("ivec2 wrapped = mod(x_local, y_local);"));
    assert!(!source.contains("ivec2 wrapped = _we_user_mod(x_local, y_local);"));
}

#[test]
fn user_mod_classification_keeps_uint_vector_shadowed_builtin_calls() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    {\n",
        "        uvec3 x = uvec3(5u);\n",
        "        uvec3 y = uvec3(2u);\n",
        "        uvec3 wrapped = mod(x, y);\n",
        "        gl_FragColor = vec4(vec3(wrapped), 1.0);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uvec3 wrapped = mod(x_local, y_local);"));
    assert!(!source.contains("uvec3 wrapped = _we_user_mod(x_local, y_local);"));
}

#[test]
fn user_mod_classification_blocks_matrix_shadowing_scalar_names() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    {\n",
        "        mat2 x = mat2(1.0);\n",
        "        mat2 y = mat2(1.0);\n",
        "        mat2 wrapped = mod(x, y);\n",
        "        gl_FragColor = vec4(wrapped[0], 0.0, 1.0);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("mat2 wrapped = mod(x_local, y_local);"));
    assert!(!source.contains("mat2 wrapped = _we_user_mod(x_local, y_local);"));
}

#[test]
fn user_mod_classification_blocks_struct_shadowing_scalar_names() {
    let source = concat!(
        "struct Payload { float value; };\n",
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    {\n",
        "        Payload x;\n",
        "        Payload y;\n",
        "        Payload wrapped = mod(x, y);\n",
        "        gl_FragColor = vec4(wrapped.value);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("Payload wrapped = mod(x, y);"));
    assert!(!source.contains("Payload wrapped = _we_user_mod(x, y);"));
}

#[test]
fn user_mod_classification_blocks_struct_typed_function_parameters() {
    let source = concat!(
        "struct Payload { float value; };\n",
        "float x = 5.5;\n",
        "float y = 2.0;\n",
        "float mod(float x, float y) { return x - y; }\n",
        "Payload helper(Payload x, Payload y) {\n",
        "    Payload wrapped = mod(x, y);\n",
        "    return wrapped;\n",
        "}\n",
        "void main() {\n",
        "    Payload x_payload;\n",
        "    Payload y_payload;\n",
        "    Payload wrapped = helper(x_payload, y_payload);\n",
        "    gl_FragColor = vec4(wrapped.value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("Payload wrapped = mod(x, y);"));
    assert!(!source.contains("Payload wrapped = _we_user_mod(x, y);"));
}

#[test]
fn user_mod_classification_does_not_assume_unknown_function_parameters_are_scalar() {
    let source = concat!(
        "float x = 5.5;\n",
        "float y = 2.0;\n",
        "float mod(float x, float y) { return x - y; }\n",
        "UnknownPayload helper(UnknownPayload x, UnknownPayload y) {\n",
        "    UnknownPayload wrapped = mod(x, y);\n",
        "    return wrapped;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x, float y)"));
    assert!(source.contains("UnknownPayload wrapped = mod(x, y);"));
    assert!(!source.contains("UnknownPayload wrapped = _we_user_mod(x, y);"));
}

#[test]
fn user_mod_classification_lets_inner_locals_shadow_function_parameters() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "float helper(float x, float y) {\n",
        "    {\n",
        "        vec2 x = vec2(5.5);\n",
        "        vec2 y = vec2(2.0);\n",
        "        return mod(x, y).x;\n",
        "    }\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(helper(5.5, 2.0));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("return mod(x, y).x;"));
    assert!(!source.contains("return _we_user_mod(x, y).x;"));
}

#[test]
fn user_mod_classification_accepts_parenthesized_and_binary_scalar_expressions() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float wrapped = mod((x), y);\n",
        "    float other = mod(x + 1.0, y);\n",
        "    gl_FragColor = vec4(wrapped + other);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float wrapped = _we_user_mod((x), y);"));
    assert!(source.contains("float other = _we_user_mod(x + 1.0, y);"));
}

#[test]
fn user_mod_classification_accepts_unary_signs_inside_scalar_expressions() {
    let source = concat!(
        "float mod(float x, float y) { return x - y; }\n",
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float product = mod(x * -y, y);\n",
        "    float offset = mod(x + -1.0, y);\n",
        "    gl_FragColor = vec4(product + offset);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float product = _we_user_mod(x * -y, y);"));
    assert!(source.contains("float offset = _we_user_mod(x + -1.0, y);"));
}

#[test]
fn builtin_two_arg_mod_stays_accessible_without_user_function() {
    let source = concat!(
        "void main() {\n",
        "    vec2 wrapped = mod(vec2(5.5), 2.0);\n",
        "    gl_FragColor = vec4(wrapped, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 wrapped = mod(vec2(5.5), 2.0);"));
    assert!(!source.contains("_we_user_mod"));
}

#[test]
fn identifier_legalization_ignores_comments_and_string_literals_but_legalizes_define_bodies() {
    let source = concat!(
        "#define TEXTURE_CALL tex2D(g_Texture0, uv)\n",
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    // tex2D and float4 inside comments must remain visible in diagnostics\n",
        "    const char* label = \"tex2D float4 gl_FragColor\";\n",
        "    float4 color = tex2D(g_Texture0, vec2(0.5));\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "#define TEXTURE_CALL texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv)"
    ));
    assert!(source.contains("// tex2D and float4 inside comments must remain visible"));
    assert!(source.contains("\"tex2D float4 gl_FragColor\""));
    assert!(source.contains(
        "vec4 color = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), vec2(0.5));"
    ));
    assert!(source.contains("_we_FragColor = color;"));
}

#[test]
fn renames_local_identifiers_that_collide_with_stage_interfaces_and_glsl_words() {
    let source = concat!(
        "#define SHOW_AND and\n",
        "varying vec2 uv;\n",
        "void main() {\n",
        "    // uv and and should stay visible in comments\n",
        "    const char* label = \"uv and\";\n",
        "    gl_FragColor = vec4(uv, 0.0, 1.0);\n",
        "    float uv = 1.0;\n",
        "    float and = uv;\n",
        "    float or = and;\n",
        "    float xor = or;\n",
        "    float not = xor;\n",
        "    gl_FragColor = vec4(and + or + xor + not);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("#define SHOW_AND and"));
    assert!(source.contains("// uv and and should stay visible in comments"));
    assert!(source.contains("\"uv and\""));
    assert!(source.contains(" in vec2 "));
    assert!(source.contains("_we_FragColor = vec4(uv, 0.0, 1.0);"));
    assert!(source.contains("float uv_local = 1.0;"));
    assert!(source.contains("float and_local = uv_local;"));
    assert!(source.contains("float or_local = and_local;"));
    assert!(source.contains("float xor_local = or_local;"));
    assert!(source.contains("float not_local = xor_local;"));
    assert!(source.contains("_we_FragColor = vec4(and_local + or_local + xor_local + not_local);"));
    assert!(!source.contains("float uv = 1.0;"));
    assert!(!source.contains("float and = uv;"));
}

#[test]
fn renames_legacy_vector_alias_locals_that_collide_with_stage_interfaces() {
    let source = concat!(
        "varying vec2 uv;\n",
        "varying vec3 normal;\n",
        "varying vec4 color;\n",
        "void main() {\n",
        "    float2 uv = float2(0.25, 0.75);\n",
        "    float3 normal = float3(0.0, 0.0, 1.0);\n",
        "    float4 color = float4(uv, normal.z, 1.0);\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 uv_local = vec2(0.25, 0.75);"));
    assert!(source.contains("vec3 normal_local = vec3(0.0, 0.0, 1.0);"));
    assert!(source.contains("vec4 color_local = vec4(uv_local, normal_local.z, 1.0);"));
    assert!(source.contains("_we_FragColor = color_local;"));
    assert!(!source.contains("vec2 uv = vec2(0.25, 0.75);"));
    assert!(!source.contains("vec3 normal = vec3(0.0, 0.0, 1.0);"));
    assert!(!source.contains("vec4 color = vec4(uv, normal.z, 1.0);"));
}

#[test]
fn renames_bool_locals_named_like_glsl_operator_words() {
    let source = concat!(
        "void main() {\n",
        "    bool and = true;\n",
        "    bool or = and;\n",
        "    bool xor = or;\n",
        "    bool not = xor;\n",
        "    gl_FragColor = vec4(not ? 1.0 : 0.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("bool and_local = true;"));
    assert!(source.contains("bool or_local = and_local;"));
    assert!(source.contains("bool xor_local = or_local;"));
    assert!(source.contains("bool not_local = xor_local;"));
    assert!(source.contains("_we_FragColor = vec4(not_local ? 1.0 : 0.0);"));
    assert!(!source.contains("bool and = true;"));
    assert!(!source.contains("bool or = and;"));
}

#[test]
fn reserved_identifier_policy_does_not_rewrite_member_access_fields() {
    let source = concat!(
        "varying vec2 uv;\n",
        "struct Material { float uv; };\n",
        "void main() {\n",
        "    Material material;\n",
        "    float uv = 1.0;\n",
        "    float sampled = material.uv + uv;\n",
        "    gl_FragColor = vec4(sampled);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float uv_local = 1.0;"));
    assert!(source.contains("float sampled = material.uv + uv_local;"));
    assert!(!source.contains("material.uv_local"));
}

#[test]
fn reserved_identifier_policy_keeps_independent_function_local_scopes_separate() {
    let source = concat!(
        "float first() {\n",
        "    float value = 1.0;\n",
        "    return value;\n",
        "}\n",
        "float second() {\n",
        "    float value = 2.0;\n",
        "    return value;\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(first() + second());\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float value = 1.0;"));
    assert!(source.contains("float value = 2.0;"));
    assert!(source.contains("return value;"));
    assert!(!source.contains("value_local"));
}

#[test]
fn reserved_identifier_policy_renames_reserved_comma_declarators() {
    let source = concat!(
        "void main() {\n",
        "    float uv = 1.0, and = uv;\n",
        "    gl_FragColor = vec4(and);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float uv = 1.0, and_local = uv;"));
    assert!(source.contains("_we_FragColor = vec4(and_local);"));
    assert!(!source.contains("vec4(and);"));
}

#[test]
fn reserved_identifier_policy_renames_same_statement_uses_after_comma_declarator() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    float uv = 1.0, x = uv;\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float uv_local = 1.0, x = uv_local;"));
    assert!(!source.contains("float uv_local = 1.0, x = uv;"));
}

#[test]
fn reserved_identifier_policy_limits_for_header_declaration_to_loop_scope() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    for (float uv = 0.0; uv < 1.0; uv += 0.1) {}\n",
        "    gl_FragColor = vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (float uv_local = 0.0; uv_local < 1.0; uv_local += 0.1) {}"));
    assert!(source.contains("_we_FragColor = vec4(uv, 0.0, 1.0);"));
    assert!(!source.contains("_we_FragColor = vec4(uv_local, 0.0, 1.0);"));
}

#[test]
fn reserved_identifier_policy_keeps_for_initializer_visible_through_if_else_body() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    for (float uv = 0.0; uv < 1.0; uv += 0.1)\n",
        "        if (uv > 0.5) gl_FragColor = vec4(uv);\n",
        "        else gl_FragColor = vec4(uv + 1.0);\n",
        "    gl_FragColor += vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (float uv_local = 0.0; uv_local < 1.0; uv_local += 0.1)"));
    assert!(source.contains("if (uv_local > 0.5) _we_FragColor = vec4(uv_local);"));
    assert!(source.contains("else _we_FragColor = vec4(uv_local + 1.0);"));
    assert!(source.contains("_we_FragColor += vec4(uv, 0.0, 1.0);"));
    assert!(!source.contains("else _we_FragColor = vec4(uv + 1.0);"));
    assert!(!source.contains("_we_FragColor += vec4(uv_local, 0.0, 1.0);"));
}

#[test]
fn reserved_identifier_policy_keeps_for_initializer_visible_through_braced_if_else_body() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    for (float uv = 0.0; uv < 1.0; uv += 0.1)\n",
        "        if (uv > 0.5) { gl_FragColor = vec4(uv); }\n",
        "        else { gl_FragColor = vec4(uv + 1.0); }\n",
        "    gl_FragColor += vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (float uv_local = 0.0; uv_local < 1.0; uv_local += 0.1)"));
    assert!(source.contains("if (uv_local > 0.5) { _we_FragColor = vec4(uv_local); }"));
    assert!(source.contains("else { _we_FragColor = vec4(uv_local + 1.0); }"));
    assert!(source.contains("_we_FragColor += vec4(uv, 0.0, 1.0);"));
    assert!(!source.contains("else { _we_FragColor = vec4(uv + 1.0); }"));
    assert!(!source.contains("_we_FragColor += vec4(uv_local, 0.0, 1.0);"));
}

#[test]
fn reserved_identifier_policy_limits_for_initializer_scope_after_nested_loop_body() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    for (float uv = 0.0; uv < 1.0; uv += 0.1)\n",
        "        for (int i = 0; i < 2; i++) { gl_FragColor = vec4(uv); }\n",
        "    gl_FragColor += vec4(uv, 0.0, 1.0);\n",
        "    for (float uv = 0.0; uv < 1.0; uv += 0.1)\n",
        "        while (uv < 0.5) { gl_FragColor += vec4(uv); break; }\n",
        "    gl_FragColor += vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "for (float uv_local = 0.0; uv_local < 1.0; uv_local += 0.1)\n        for (int i = \
         int(0); i < int(2); i++) { _we_FragColor = vec4(uv_local); }"
    ));
    assert!(source.contains(
        "for (float uv_local_1 = 0.0; uv_local_1 < 1.0; uv_local_1 += 0.1)\n        while \
         (uv_local_1 < 0.5) { _we_FragColor += vec4(uv_local_1); break; }"
    ));
    assert_eq!(
        source
            .matches("_we_FragColor += vec4(uv, 0.0, 1.0);")
            .count(),
        2
    );
    assert!(!source.contains("_we_FragColor += vec4(uv_local, 0.0, 1.0);"));
    assert!(!source.contains("_we_FragColor += vec4(uv_local_1, 0.0, 1.0);"));
}

#[test]
fn promotes_vec4_texture_initializers_assigned_to_scalar_variables() {
    let source = concat!(
        "uniform sampler2D g_Texture1;\n",
        "void main() {\n",
        "    float alpha = texture2D(g_Texture1, vec2(0.5));\n",
        "    gl_FragColor = vec4(alpha);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);

    assert!(legalized.source().contains(
        "float alpha = (texture(sampler2D(g_Texture1, _we_Sampler_g_Texture1), vec2(0.5))).x;"
    ));
}

#[test]
fn scalar_texture_promotion_wraps_the_full_initializer_rhs() {
    let source = concat!(
        "uniform sampler2D g_Texture1;\n",
        "void main() {\n",
        "    float alpha = texture2D(g_Texture1, vec2(0.5)) + vec4(0.25);\n",
        "    gl_FragColor = vec4(alpha);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);

    assert!(legalized.source().contains(
        "float alpha = (texture(sampler2D(g_Texture1, _we_Sampler_g_Texture1), vec2(0.5)) + \
         vec4(0.25)).x;"
    ));
}

#[test]
fn workshop_3611439897_sharpen_style_vec4_rhs_is_reduced_before_scalar_assignment() {
    let source = concat!(
        "uniform sampler2D g_Texture1;\n",
        "void main() {\n",
        "    vec2 uv = vec2(0.5);\n",
        "    vec4 sharpen = texture2D(g_Texture1, uv) + vec4(0.125);\n",
        "    float sharpen_alpha = sharpen;\n",
        "    gl_FragColor = vec4(sharpen_alpha);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float sharpen_alpha = sharpen.x;"));
    assert!(!source.contains("float sharpen_alpha = sharpen;"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("Sharpen-style scalar assignment reduction should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn workshop_3611439897_sharpen_style_numeric_condition_is_made_boolean() {
    let source = concat!(
        "void main() {\n",
        "    float sharpen_amount = 0.75;\n",
        "    if (sharpen_amount) {\n",
        "        gl_FragColor = vec4(sharpen_amount);\n",
        "    } else {\n",
        "        gl_FragColor = vec4(0.0);\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("if (sharpen_amount != 0.0) {"));
    assert!(!source.contains("if (sharpen_amount) {"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("Sharpen-style numeric condition reduction should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn workshop_3611439897_sharpen_style_numeric_ternary_condition_is_made_boolean() {
    let source = concat!(
        "#define INVERT 0\n",
        "uniform sampler2D g_Texture0;\n",
        "uniform sampler2D g_Texture1;\n",
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 albedo = texSample2D(g_Texture0, v_TexCoord.xy);\n",
        "    float mask = texSample2D(g_Texture1, v_TexCoord.xy);\n",
        "    mask = INVERT ? 1 - mask : mask;\n",
        "    gl_FragColor = albedo * mask;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("mask = INVERT != 0 ? 1 - mask : mask;"));
    assert!(!source.contains("mask = INVERT ? 1 - mask : mask;"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("Sharpen-style numeric ternary condition should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn control_flow_coercion_policy_numeric_ternary_condition_preserves_nested_texture_sampling() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 a = vec4(1.0);\n",
        "    vec4 b = vec4(0.0);\n",
        "    gl_FragColor = texSample2D(g_Texture0, v_TexCoord.xy).x ? a : b;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "_we_FragColor = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
             v_TexCoord.xy).x != 0.0 ? a : b;"
        ),
        "{source}"
    );
    assert!(!source.contains("texSample2D("));
}

#[test]
fn control_flow_coercion_policy_preserves_boolean_comparison_ternary_condition() {
    let source = concat!(
        "vec3 apply_blending(int mode, vec3 base, vec3 tint, float mask) {\n",
        "    return mode == 30 ? mix(base, tint, mask) : base;\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(apply_blending(30, vec3(0.0), vec3(1.0), 0.5), 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("return mode == 30 ? mix(base, tint, mask) : base;"));
    assert!(!source.contains("mode == 30 != 0"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("boolean comparison ternary condition should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_narrows_vector_texture_initializers() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    vec2 uv = texture2D(g_Texture0, vec2(0.5));\n",
        "    vec3 normal = texSample2D(g_Texture0, uv);\n",
        "    gl_FragColor = vec4(normal, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec2 uv = (texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), vec2(0.5))).xy;"
    ));
    assert!(source.contains(
        "vec3 normal = (texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv)).xyz;"
    ));
}

#[test]
fn legalized_texture_sampling_compiles_with_naga() {
    let source = concat!(
        "varying vec2 v_Uv;\n",
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    gl_FragColor = texture2D(g_Texture0, v_Uv);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("legalized fragment texture sampling should compile through Naga");

    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn legalizes_hlsl_mul_vertex_transform_for_naga() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "uniform mat4 g_ModelViewProjectionMatrix;\n",
        "void main() {\n",
        "    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);

    assert!(
        legalized
            .source()
            .contains("gl_Position = ((g_ModelViewProjectionMatrix) * (vec4(a_Position, 1.0)));")
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("legalized vertex HLSL mul should compile through Naga");

    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
}

#[test]
fn legalizes_nested_hlsl_mul_vertex_transform_for_naga() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "uniform mat4 g_AltViewProjectionMatrix;\n",
        "uniform mat4 g_ViewProjectionMatrix;\n",
        "void main() {\n",
        "    gl_Position = mul(mul(vec4(a_Position, 1.0), g_AltViewProjectionMatrix), \
         g_ViewProjectionMatrix);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);

    assert!(legalized.source().contains(
        "gl_Position = ((g_ViewProjectionMatrix) * (((g_AltViewProjectionMatrix) * \
         (vec4(a_Position, 1.0)))));"
    ));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("legalized nested vertex HLSL mul should compile through Naga");

    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
}

#[test]
fn explicit_hlsl_mul_rewrite_does_not_emit_compatibility_macro() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "uniform mat4 g_AltViewProjectionMatrix;\n",
        "void main() {\n",
        "    gl_Position = mul(vec4(a_Position, 1.0), g_AltViewProjectionMatrix);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(
        source.contains("gl_Position = ((g_AltViewProjectionMatrix) * (vec4(a_Position, 1.0)));")
    );
    assert!(!source.contains("#define mul"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("syntax-rewritten HLSL mul should compile through Naga");

    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
}

#[test]
fn policies_support_legacy_texture_lod_and_clip_without_macro_prelude() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    float alpha = texSample2DLod(g_Texture0, vec2(0.5), 0.0).r;\n",
        "    clip(alpha - 0.1);\n",
        "    gl_FragColor = CAST4(frac(alpha));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(!source.contains("#define texSample2DLod"));
    assert!(source.contains(
        "float alpha = textureLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), vec2(0.5), \
         0.0).r;"
    ));
    assert!(source.contains("void clip(float value)"));
    assert!(source.contains("_we_FragColor = vec4(fract(alpha));"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("legacy textureLod and clip helpers should compile through Naga");

    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn legacy_compatibility_macros_are_not_emitted_when_policy_rewrites_apply() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "uniform mat4 g_ModelViewProjectionMatrix;\n",
        "void main() {\n",
        "    float2 uv = float2(0.25, 0.75);\n",
        "    float4 sampled = texSample2D(g_Texture0, uv);\n",
        "    float4 color = lerp(float4(0.0), sampled, saturate(0.5));\n",
        "    gl_FragColor = mul(color, g_ModelViewProjectionMatrix);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 uv = vec2(0.25, 0.75);"));
    assert!(
        source
            .contains("vec4 sampled = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv);")
    );
    assert!(source.contains("vec4 color = mix(vec4(0.0), sampled, clamp(0.5, 0.0, 1.0));"));
    assert!(source.contains("_we_FragColor = ((g_ModelViewProjectionMatrix) * (color));"));
    assert!(!source.contains("#define mul"));
    assert!(!source.contains("#define saturate"));
    assert!(!source.contains("#define lerp"));
    assert!(!source.contains("#define float4"));
    assert!(!source.contains("#define texSample2D"));
}

#[test]
fn compatibility_functions_emit_only_when_referenced() {
    let source_without_helpers =
        concat!("void main() {\n", "    gl_FragColor = vec4(1.0);\n", "}\n",);

    let legalized = legalize(ShaderStageKind::Fragment, source_without_helpers);
    let source = legalized.source();

    assert!(!source.contains("void clip("));
    assert!(!source.contains("vec3 PerformLighting_V1("));

    let source_with_helpers = concat!(
        "void main() {\n",
        "    vec3 lit = PerformLighting_V1(vec3(0.0), vec3(1.0), vec3(0.0, 0.0, 1.0), vec3(0.0, \
         0.0, 1.0), vec3(1.0), vec3(0.04), 0.5, 0.0);\n",
        "    clip(lit.r - 0.1);\n",
        "    gl_FragColor = vec4(lit, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source_with_helpers);
    let source = legalized.source();

    assert!(source.contains("void clip(float value)"));
    assert!(source.contains("vec3 PerformLighting_V1("));
}

#[test]
fn compatibility_functions_do_not_duplicate_user_defined_clip() {
    let source = concat!(
        "void clip(float value) {\n",
        "    if (value > 1.0) { discard; }\n",
        "}\n",
        "void main() {\n",
        "    float alpha = 0.5;\n",
        "    clip(alpha);\n",
        "    gl_FragColor = vec4(alpha);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("void clip(float value)"));
    assert!(source.contains("clip(alpha);"));
    assert!(!source.contains("value < 0.0"));
}

#[test]
fn compatibility_functions_do_not_duplicate_user_defined_perform_lighting() {
    let source = concat!(
        "vec3 PerformLighting_V1(vec3 color) {\n",
        "    return color * 0.5;\n",
        "}\n",
        "void main() {\n",
        "    vec3 lit = PerformLighting_V1(vec3(1.0));\n",
        "    gl_FragColor = vec4(lit, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 PerformLighting_V1(vec3 color)"));
    assert!(source.contains("vec3 lit = PerformLighting_V1(vec3(1.0));"));
    assert!(!source.contains("vec3 world_pos"));
}

#[test]
fn perform_lighting_compatibility_function_is_available_to_vertex_stage() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "void main() {\n",
        "    vec3 lit = PerformLighting_V1(a_Position, vec3(1.0), vec3(0.0, 0.0, 1.0), vec3(0.0, \
         0.0, 1.0), vec3(1.0), vec3(0.04), 0.5, 0.0);\n",
        "    gl_Position = vec4(lit, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(source.contains("vec3 PerformLighting_V1("));
    assert!(!source.contains("void clip("));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("legacy vertex PerformLighting_V1 helper should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
}

#[test]
fn clip_compatibility_function_stays_fragment_only() {
    let source = concat!(
        "attribute vec3 a_Position;\n",
        "void main() {\n",
        "    clip(a_Position.x - 0.1);\n",
        "    gl_Position = vec4(a_Position, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(!source.contains("void clip("));
}

#[test]
fn legacy_builtin_policy_handles_general_call_rewrites() {
    let source = concat!(
        "#define LEGACY_NAMES frac log10 atan2 fmod ddx ddy saturate lerp\n",
        "void main() {\n",
        "    // frac log10 atan2 fmod ddx ddy saturate lerp stay as diagnostic text\n",
        "    const char* names = \"frac log10 atan2 fmod ddx ddy saturate lerp\";\n",
        "    float a = frac(1.25);\n",
        "    float b = log10(100.0);\n",
        "    float c = atan2(1.0, 2.0);\n",
        "    float d = fmod(5.5, 2.0);\n",
        "    float e = ddx(a);\n",
        "    float f = ddy(b);\n",
        "    float g = saturate(c);\n",
        "    float h = lerp(d, e, 0.5);\n",
        "    gl_FragColor = vec4(a + b + c + d + e + f + g + h);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("#define LEGACY_NAMES frac log10 atan2 fmod ddx ddy saturate lerp"));
    assert!(source.contains("// frac log10 atan2 fmod ddx ddy saturate lerp stay"));
    assert!(source.contains("\"frac log10 atan2 fmod ddx ddy saturate lerp\""));
    assert!(source.contains("float a = fract(1.25);"));
    assert!(source.contains("float b = (log2(100.0) * 0.301029995663981);"));
    assert!(source.contains("float c = atan(1.0, 2.0);"));
    assert!(source.contains("float d = ((5.5) - (2.0) * trunc((5.5) / (2.0)));"));
    assert!(source.contains("float e = dFdx(a);"));
    assert!(source.contains("float f = dFdy(-(b));"));
    assert!(source.contains("float g = clamp(c, 0.0, 1.0);"));
    assert!(source.contains("float h = mix(d, e, 0.5);"));
}

#[test]
fn type_coercion_policy_promotes_integer_literals_for_float_builtins() {
    let source = concat!(
        "void main() {\n",
        "    float a = mix(0, 1, 1);\n",
        "    float b = smoothstep(0, 1, 1);\n",
        "    float c = step(0, 1);\n",
        "    float d = pow(2, 3);\n",
        "    float e = clamp(1, 0, 2);\n",
        "    gl_FragColor = vec4(a + b + c + d + e);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float a = mix(0.0, 1.0, 1.0);"));
    assert!(source.contains("float b = smoothstep(0.0, 1.0, 1.0);"));
    assert!(source.contains("float c = step(0.0, 1.0);"));
    assert!(source.contains("float d = pow(2.0, 3.0);"));
    assert!(source.contains("float e = clamp(1.0, 0.0, 2.0);"));
}

#[test]
fn type_coercion_policy_broadcasts_scalar_arguments_to_vector_width() {
    let source = concat!(
        "void main() {\n",
        "    vec2 a = mix(vec2(0.0), 1, 0.5);\n",
        "    vec3 b = pow(2, vec3(3.0));\n",
        "    vec4 c = clamp(vec4(1.0), 0, 2);\n",
        "    vec3 d = max(0, vec3(1.0));\n",
        "    gl_FragColor = vec4(a.x + b.x + c.x + d.x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 a = mix(vec2(0.0), vec2(1.0), 0.5);"));
    assert!(source.contains("vec3 b = pow(vec3(2.0), vec3(3.0));"));
    assert!(source.contains("vec4 c = clamp(vec4(1.0), vec4(0.0), vec4(2.0));"));
    assert!(source.contains("vec3 d = max(vec3(0.0), vec3(1.0));"));
}

#[test]
fn type_coercion_policy_broadcasts_scalar_expression_in_vector_max() {
    let source = concat!(
        "float luma(vec3 color) {\n",
        "    return dot(color, vec3(0.299, 0.587, 0.114));\n",
        "}\n",
        "void main() {\n",
        "    vec3 color = vec3(1.25, 1.0, 0.75);\n",
        "    color += max(luma(color) - 1.0, vec3(0.0));\n",
        "    gl_FragColor = vec4(color, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color += max(vec3(luma(color) - 1.0), vec3(0.0));"));
    assert!(!source.contains("color += max(luma(color) - 1.0, vec3(0.0));"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vector max with scalar expression operand should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn workshop_3212731906_hue_shift_mix_narrows_wide_peer_for_naga() {
    let source = concat!(
        "void main() {\n",
        "    vec4 albedo = vec4(0.1, 0.2, 0.3, 1.0);\n",
        "    vec3 newAlbedo = vec3(0.3, 0.2, 0.1);\n",
        "    float mask = 0.5;\n",
        "    albedo.rgb = mix(albedo, newAlbedo, mask);\n",
        "    gl_FragColor = albedo;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("albedo.rgb = mix(albedo.xyz, newAlbedo, mask);"),
        "workshop 3212731906 hue_shift mix should narrow albedo to vec3; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("workshop 3212731906 hue_shift mixed-width mix should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_narrows_vector_mix_mask_and_step_edge_arguments() {
    let source = concat!(
        "void main() {\n",
        "    vec4 wide_a = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 wide_b = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    vec4 wide_mask = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    vec3 mixed = mix(wide_a, wide_b, wide_mask);\n",
        "    vec4 wide_edge = vec4(0.2, 0.4, 0.6, 0.8);\n",
        "    vec4 wide_value = vec4(0.1, 0.5, 0.7, 0.9);\n",
        "    vec3 stepped = step(wide_edge, wide_value);\n",
        "    gl_FragColor = vec4(mixed + stepped, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 mixed = mix(wide_a.xyz, wide_b.xyz, wide_mask.xyz);"),
        "vector mix arguments, including vector alpha, should narrow to vec3; source:\n{source}"
    );
    assert!(
        source.contains("vec3 stepped = step(wide_edge.xyz, wide_value.xyz);"),
        "vector step arguments, including vector edge, should narrow to vec3; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("mixed-width vector mix/step calls should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_does_not_narrow_mix_below_declared_vec4_context() {
    let source = concat!(
        "void main() {\n",
        "    vec4 wide4 = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec3 narrow3 = vec3(0.5, 0.6, 0.7);\n",
        "    float mask = 0.5;\n",
        "    vec4 outv = mix(wide4, narrow3, mask);\n",
        "    gl_FragColor = outv;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec4 outv = mix(wide4, narrow3, mask);"),
        "declared vec4 context should not be lowered to the narrower vec3 peer; source:\n{source}"
    );
    assert!(
        !source.contains("vec4 outv = mix(wide4.xyz, narrow3, mask);"),
        "declared vec4 context must not receive a vec3 RHS; source:\n{source}"
    );
}

#[test]
fn type_coercion_policy_does_not_apply_lhs_context_to_swizzled_call_result() {
    let source = concat!(
        "void main() {\n",
        "    vec4 a4 = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 b4 = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    float t = 0.5;\n",
        "    vec3 outv = mix(a4, b4, t).gba;\n",
        "    gl_FragColor = vec4(outv, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 outv = mix(a4, b4, t).gba;"),
        "swizzled call result should not narrow the underlying vec4 call arguments; \
         source:\n{source}"
    );
    assert!(
        !source.contains("mix(a4.xyz, b4.xyz, t).gba"),
        "lhs vec3 context must not leave .gba on a narrowed vec3 call result; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("swizzled vec4 mix result should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_uses_declaration_width_before_shadowed_outer_binding() {
    let source = concat!(
        "vec4 color = vec4(0.0);\n",
        "void main() {\n",
        "    vec4 wide_a = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 wide_b = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    vec4 wide_mask = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    vec3 color = mix(wide_a, wide_b, wide_mask);\n",
        "    gl_FragColor = vec4(color, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 color = mix(wide_a.xyz, wide_b.xyz, wide_mask.xyz);"),
        "declaration target width should win over shadowed outer binding; source:\n{source}"
    );
    assert!(!source.contains("vec3 color = mix(wide_a, wide_b, wide_mask);"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("shadowed declaration-width mix should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_uses_special_vector_arguments_as_fallback_width() {
    let source = concat!(
        "void main() {\n",
        "    vec3 mask_vec3 = vec3(0.25, 0.5, 0.75);\n",
        "    vec3 m = mix(0.0, 1.0, mask_vec3);\n",
        "    vec3 edge_vec3 = vec3(0.2, 0.4, 0.6);\n",
        "    vec3 s = step(edge_vec3, 0.5);\n",
        "    gl_FragColor = vec4(m + s, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 m = mix(vec3(0.0), vec3(1.0), mask_vec3);"),
        "vector mix alpha should provide fallback width for scalar peers; source:\n{source}"
    );
    assert!(
        source.contains("vec3 s = step(edge_vec3, vec3(0.5));"),
        "vector step edge should provide fallback width for scalar value; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("special vector mix/step arguments should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_uses_special_vector_arguments_without_context_width() {
    let source = concat!(
        "void main() {\n",
        "    vec3 mask_vec3 = vec3(0.25, 0.5, 0.75);\n",
        "    vec3 edge_vec3 = vec3(0.2, 0.4, 0.6);\n",
        "    gl_FragColor = vec4(mix(0.0, 1.0, mask_vec3) + step(edge_vec3, 0.5), 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "vec4(mix(vec3(0.0), vec3(1.0), mask_vec3) + step(edge_vec3, vec3(0.5)), 1.0)"
        ),
        "vector mix alpha and step edge should provide fallback width without assignment context; \
         source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("special vector mix/step arguments without context should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_uses_special_vector_width_with_primary_width_without_context() {
    let source = concat!(
        "void main() {\n",
        "    vec4 wide4 = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec3 mask3 = vec3(0.25, 0.5, 0.75);\n",
        "    vec3 edge3 = vec3(0.2, 0.4, 0.6);\n",
        "    gl_FragColor = vec4(mix(wide4, 0.0, mask3) + step(edge3, wide4), 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec4(mix(wide4.xyz, vec3(0.0), mask3) + step(edge3, wide4.xyz), 1.0)"),
        "primary and special vector widths should select the compatible minimum without direct \
         assignment context; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect(
            "mixed primary/special vector arguments without context should compile through Naga",
        );
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_does_not_infer_swizzle_width_for_struct_field_lvalues() {
    let source = concat!(
        "struct Surface { vec4 rgb; };\n",
        "void main() {\n",
        "    Surface surface;\n",
        "    vec3 rgb = vec3(0.0);\n",
        "    vec4 a = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 b = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    vec4 mask = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    surface.rgb = mix(a, b, mask);\n",
        "    gl_FragColor = surface.rgb + vec4(rgb, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("surface.rgb = mix(a, b, mask);"),
        "struct field named rgb should not be treated as a vec3 swizzle lvalue; source:\n{source}"
    );
    assert!(!source.contains("surface.rgb = mix(a.xyz, b.xyz, mask"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("struct field lvalue assignment should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_blocks_outer_vector_base_with_shadowed_struct_lvalue() {
    let source = concat!(
        "struct Surface { vec4 rgb; };\n",
        "vec4 surface = vec4(0.0);\n",
        "void main() {\n",
        "    Surface surface;\n",
        "    vec4 rgb = vec4(0.0);\n",
        "    vec4 a = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 b = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    vec4 mask = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    surface.rgb = mix(a, b, mask);\n",
        "    gl_FragColor = surface.rgb + rgb;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("surface.rgb = mix(a, b, mask);"),
        "shadowed struct lvalue should block the outer vector base; source:\n{source}"
    );
    assert!(
        !source.contains("surface.rgb = mix(a.xyz, b.xyz, mask"),
        "outer vec4 surface binding must not cause RHS narrowing; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("shadowed struct lvalue assignment should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_policy_does_not_infer_chained_member_width_from_unrelated_field_name() {
    let source = concat!(
        "struct Payload { vec4 rgb; };\n",
        "struct Material { Payload payload; };\n",
        "vec4 payload = vec4(0.0);\n",
        "void main() {\n",
        "    Material material;\n",
        "    vec4 payload = vec4(1.0);\n",
        "    vec4 a = vec4(0.1, 0.2, 0.3, 0.4);\n",
        "    vec4 b = vec4(0.5, 0.6, 0.7, 0.8);\n",
        "    vec4 mask = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    material.payload.rgb = mix(a, b, mask);\n",
        "    gl_FragColor = material.payload.rgb + payload;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("material.payload.rgb = mix(a, b, mask);"),
        "chained member access must not infer width from unrelated payload binding; \
         source:\n{source}"
    );
    assert!(
        !source.contains("material.payload.rgb = mix(a.xyz, b.xyz, mask"),
        "unrelated payload vector binding must not cause RHS narrowing; source:\n{source}"
    );
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("chained member lvalue assignment should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn legacy_lerp_rewrite_composes_with_type_coercion() {
    let source = concat!(
        "void main() {\n",
        "    vec2 color = lerp(vec2(0.0), 1, 0.5);\n",
        "    gl_FragColor = vec4(color, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 color = mix(vec2(0.0), vec2(1.0), 0.5);"));
    assert!(!source.contains("lerp("));
}

#[test]
fn type_coercion_policy_swizzles_vec4_identifier_initializers_for_narrow_vectors() {
    let source = concat!(
        "void main() {\n",
        "    vec4 packed = vec4(1.0, 2.0, 3.0, 4.0);\n",
        "    vec2 uv = packed;\n",
        "    vec3 normal = packed;\n",
        "    vec4 color = packed;\n",
        "    gl_FragColor = color + vec4(uv, normal.z, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 uv = packed.xy;"));
    assert!(source.contains("vec3 normal = packed.xyz;"));
    assert!(source.contains("vec4 color = packed;"));
}

#[test]
fn type_coercion_policy_uses_nearest_binding_for_narrow_vector_identifier_initializers() {
    let source = concat!(
        "void main() {\n",
        "    vec4 packed = vec4(1.0, 2.0, 3.0, 4.0);\n",
        "    vec2 uv = vec2(0.0);\n",
        "    {\n",
        "        float packed = 0.5;\n",
        "        vec2 inner = packed;\n",
        "        uv += vec2(packed);\n",
        "    }\n",
        "    gl_FragColor = vec4(uv, packed.zw);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float packed_local = 0.5;"));
    assert!(source.contains("vec2 inner = packed_local;"));
    assert!(!source.contains("vec2 inner = packed_local.xy;"));
}

#[test]
fn type_coercion_policy_swizzles_vec4_identifier_initializers_in_comma_declarators() {
    let source = concat!(
        "void main() {\n",
        "    vec4 packed = vec4(1.0, 2.0, 3.0, 4.0);\n",
        "    vec2 a = packed, b = packed;\n",
        "    vec2 c = vec2(0.0), d = packed;\n",
        "    gl_FragColor = vec4(a + b + c + d, packed.zw);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 a = packed.xy, b = packed.xy;"));
    assert!(source.contains("vec2 c = vec2(0.0), d = packed.xy;"));
}

#[test]
fn type_coercion_policy_broadcasts_scalar_initializers_in_multi_vector_declarations() {
    let source = concat!(
        "void main() {\n",
        "    vec3 offset = vec3(1.0), velocity = 2.0, acceleration = -3;\n",
        "    vec2 uv = 0.5, scale = vec2(1.0);\n",
        "    gl_FragColor = vec4(offset + velocity + acceleration, uv.x + scale.y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec3 offset = vec3(1.0), velocity = vec3(2.0), acceleration = vec3(-3);")
    );
    assert!(source.contains("vec2 uv = vec2(0.5), scale = vec2(1.0);"));
}

#[test]
fn control_flow_coercion_policy_lowers_float_modulo_assignments() {
    let source = concat!(
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    vec4 color = vec4(5.5);\n",
        "    float a = 5.5 % 2.0;\n",
        "    color.x = a % 2.0;\n",
        "    x %= y;\n",
        "    color.x %= y;\n",
        "    gl_FragColor = color + vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float a = fmod(5.5, 2.0);"));
    assert!(source.contains("color.x = fmod(a, 2.0);"));
    assert!(source.contains("x = ((x) - (y) * trunc((x) / (y)));"));
    assert!(source.contains("color.x = ((color.x) - (y) * trunc((color.x) / (y)));"));
}

#[test]
fn control_flow_coercion_policy_lowers_float_modulo_assignment_without_builtin_fmod() {
    let source = concat!(
        "void main() {\n",
        "    float fragLV = 3.0;\n",
        "    fragLV %= 2;\n",
        "    gl_FragColor = vec4(fragLV);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("fragLV = ((fragLV) - (2) * trunc((fragLV) / (2)));"));
    assert!(!source.contains("fmod("));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("lowered float modulo assignment should compile without fmod helper");
}

#[test]
fn control_flow_coercion_policy_modulo_assignment_preserves_nested_texture_sampling() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    float frag = 3.0;\n",
        "    frag %= texSample2D(g_Texture0, v_TexCoord.xy).x;\n",
        "    gl_FragColor = vec4(frag);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "frag = ((frag) - (texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
         v_TexCoord.xy).x) * trunc((frag) / (texture(sampler2D(g_Texture0, \
         _we_Sampler_g_Texture0), v_TexCoord.xy).x)));"
    ));
    assert!(!source.contains("texSample2D("));
}

#[test]
fn control_flow_coercion_policy_does_not_lower_unknown_or_integer_member_modulo_assignments() {
    let source = concat!(
        "struct Payload { int x; };\n",
        "void main() {\n",
        "    Payload payload;\n",
        "    ivec4 counts = ivec4(5);\n",
        "    vec4 color = vec4(5.5);\n",
        "    int y = 2;\n",
        "    payload.x %= y;\n",
        "    counts.x %= y;\n",
        "    color.x %= 2.0;\n",
        "    gl_FragColor = color + vec4(float(payload.x + counts.x));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("payload.x %= y;"));
    assert!(source.contains("counts.x %= y;"));
    assert!(source.contains("color.x = ((color.x) - (2.0) * trunc((color.x) / (2.0)));"));
    assert!(!source.contains("payload.x = fmod(payload.x, y);"));
    assert!(!source.contains("counts.x = fmod(counts.x, y);"));
}

#[test]
fn control_flow_coercion_policy_respects_integer_vector_shadowing_for_member_modulo_assignment() {
    let source = concat!(
        "vec4 color;\n",
        "void main() {\n",
        "    {\n",
        "        ivec4 color = ivec4(5);\n",
        "        color.x %= 2;\n",
        "    }\n",
        "    color.x %= 2.0;\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color.x %= 2;"));
    assert!(source.contains("color.x = ((color.x) - (2.0) * trunc((color.x) / (2.0)));"));
    assert!(!source.contains("color.x = fmod(color.x, 2);"));
}

#[test]
fn control_flow_coercion_policy_keeps_for_initializer_blocker_through_if_else_body() {
    let source = concat!(
        "vec4 color;\n",
        "void main() {\n",
        "    bool enabled = true;\n",
        "    for (ivec4 color = ivec4(5); enabled; enabled = false)\n",
        "        if (enabled)\n",
        "            color.x %= 2;\n",
        "        else\n",
        "            color.x %= 3;\n",
        "    color.x %= 2.0;\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color.x %= 2;"));
    assert!(source.contains("color.x %= 3;"));
    assert!(source.contains("color.x = ((color.x) - (2.0) * trunc((color.x) / (2.0)));"));
    assert!(!source.contains("color.x = fmod(color.x, 2);"));
    assert!(!source.contains("color.x = fmod(color.x, 3);"));
}

#[test]
fn control_flow_coercion_policy_respects_matrix_shadowing_for_member_modulo_assignment() {
    let source = concat!(
        "vec4 color;\n",
        "void main() {\n",
        "    {\n",
        "        mat2x3 color = mat2x3(1.0);\n",
        "        color.x %= 2;\n",
        "    }\n",
        "    {\n",
        "        mat4x4 color = mat4x4(1.0);\n",
        "        color.x %= 3;\n",
        "    }\n",
        "    color.x %= 2.0;\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color.x %= 2;"));
    assert!(source.contains("color_local.x %= 3;"));
    assert!(source.contains("color.x = ((color.x) - (2.0) * trunc((color.x) / (2.0)));"));
    assert!(!source.contains("color.x = fmod(color.x, 2);"));
    assert!(!source.contains("color_local.x = fmod(color_local.x, 3);"));
}

#[test]
fn control_flow_coercion_policy_respects_struct_shadowing_for_member_modulo_assignment() {
    let source = concat!(
        "struct Payload { int x; };\n",
        "vec4 payload;\n",
        "void main() {\n",
        "    Payload payload;\n",
        "    payload.x %= 2;\n",
        "    gl_FragColor = vec4(float(payload.x));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("payload.x %= 2;"));
    assert!(!source.contains("payload.x = fmod(payload.x, 2);"));
}

#[test]
fn control_flow_coercion_policy_respects_struct_parameter_shadowing_for_member_modulo_assignment() {
    let source = concat!(
        "struct Payload { int x; };\n",
        "vec4 color;\n",
        "void helper(Payload color) {\n",
        "    color.x %= 2;\n",
        "}\n",
        "void main() {\n",
        "    color.x %= 2.0;\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color.x %= 2;"));
    assert!(source.contains("color.x = ((color.x) - (2.0) * trunc((color.x) / (2.0)));"));
    assert!(!source.contains("color.x = fmod(color.x, 2);"));
}

#[test]
fn control_flow_coercion_policy_lowers_modulo_in_comma_declarators_independently() {
    let source = concat!(
        "void main() {\n",
        "    float x = 5.5;\n",
        "    float y = 2.0;\n",
        "    float a = 0.0, b = x % y, c = (x + 1.0) % y;\n",
        "    gl_FragColor = vec4(a + b + c);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float a = 0.0, b = fmod(x, y), c = fmod((x + 1.0), y);"));
    assert!(!source.contains("fmod(0.0, b = x % y"));
}

#[test]
fn control_flow_coercion_policy_lowers_modulo_operands_without_swallowing_neighbors() {
    let source = concat!(
        "void main() {\n",
        "    float a = 5.5;\n",
        "    float b = 2.0;\n",
        "    float c = 1.0;\n",
        "    float left = a + b % c;\n",
        "    float right = a % b + c;\n",
        "    gl_FragColor = vec4(left + right);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float left = a + fmod(b, c);"));
    assert!(source.contains("float right = fmod(a, b) + c;"));
    assert!(!source.contains("float left = fmod(a + b, c);"));
    assert!(!source.contains("float right = fmod(a, b + c);"));
}

#[test]
fn control_flow_coercion_policy_lowers_chained_modulo_left_associatively() {
    let source = concat!(
        "void main() {\n",
        "    float a = 5.5;\n",
        "    float b = 2.0;\n",
        "    float c = 1.0;\n",
        "    float value = a % b % c;\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float value = fmod(fmod(a, b), c);"));
    assert!(!source.contains("float value = fmod(a, b % c);"));
}

#[test]
fn control_flow_coercion_policy_lowers_parenthesized_argument_and_nested_modulo() {
    let source = concat!(
        "float passthrough(float value) { return value; }\n",
        "void main() {\n",
        "    float a = 5.5;\n",
        "    float b = 2.0;\n",
        "    float c = 1.0;\n",
        "    float x = (a % b);\n",
        "    float y = passthrough(a % b);\n",
        "    float z = a % (b % c);\n",
        "    gl_FragColor = vec4(x + y + z);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = (fmod(a, b));"));
    assert!(source.contains("float y = passthrough(fmod(a, b));"));
    assert!(source.contains("float z = fmod(a, (fmod(b, c)));"));
    assert!(!source.contains("a % b"));
    assert!(!source.contains("b % c"));
}

#[test]
fn control_flow_coercion_policy_lowers_modulo_with_signed_rhs_operands() {
    let source = concat!(
        "void main() {\n",
        "    float a = 5.5;\n",
        "    float b = 2.0;\n",
        "    float x = a % -2.0;\n",
        "    float y = a % -b;\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = fmod(a, -2.0);"));
    assert!(source.contains("float y = fmod(a, -b);"));
    assert!(!source.contains("a % -2.0"));
    assert!(!source.contains("a % -b"));
}

#[test]
fn control_flow_coercion_policy_preserves_integer_modulo_in_integer_operands() {
    let source = concat!(
        "void main() {\n",
        "    int i = 5;\n",
        "    int j = 2;\n",
        "    float x = float(i % j);\n",
        "    float y = float(uint(i % j));\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = float(i % j);"));
    assert!(source.contains("float y = float(uint(i % j));"));
    assert!(!source.contains("float(fmod(i, j))"));
    assert!(!source.contains("uint(fmod(i, j))"));
}

#[test]
fn control_flow_coercion_policy_preserves_uint_constructor_integer_modulo() {
    let source = concat!(
        "void main() {\n",
        "    int i = 5;\n",
        "    int j = 2;\n",
        "    float x = float(uint(i % j));\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = float(uint(i % j));"));
    assert!(!source.contains("uint(fmod(i, j))"));
}

#[test]
fn control_flow_coercion_policy_lowers_uint_constructor_float_modulo() {
    let source = concat!(
        "void main() {\n",
        "    float f = 5.5;\n",
        "    float g = 2.0;\n",
        "    float out_value = float(uint(f % g));\n",
        "    gl_FragColor = vec4(out_value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float out_value = float(uint(fmod(f, g)));"));
    assert!(!source.contains("uint(f % g)"));
}

#[test]
fn control_flow_coercion_policy_composes_assignment_rhs_and_constructor_modulo_lowering() {
    let source = concat!(
        "void main() {\n",
        "    float f = 5.5;\n",
        "    float g = 2.0;\n",
        "    float h = 3.0;\n",
        "    float x = 0.0;\n",
        "    x = uint(f % g) + (f % h);\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("x = uint(fmod(f, g)) + (fmod(f, h));"));
    assert!(!source.contains("uint(f % g)"));
    assert!(!source.contains("f % h"));
}

#[test]
fn control_flow_coercion_policy_lowers_uint_declaration_constructor_float_modulo() {
    let source = concat!(
        "void main() {\n",
        "    float f = 5.5;\n",
        "    float g = 2.0;\n",
        "    uint out_value = uint(f % g);\n",
        "    gl_FragColor = vec4(out_value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uint out_value = uint(fmod(f, g));"));
    assert!(!source.contains("uint(f % g)"));
}

#[test]
fn control_flow_coercion_policy_lowers_int_declaration_constructor_float_modulo() {
    let source = concat!(
        "void main() {\n",
        "    float f = 5.5;\n",
        "    float g = 2.0;\n",
        "    int out_value = int(f % g);\n",
        "    gl_FragColor = vec4(out_value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int out_value = int(fmod(f, g));"));
    assert!(!source.contains("int(f % g)"));
}

#[test]
fn control_flow_coercion_policy_preserves_uint_declaration_constructor_integer_modulo() {
    let source = concat!(
        "void main() {\n",
        "    int i = 5;\n",
        "    int j = 2;\n",
        "    uint out_value = uint(i % j);\n",
        "    gl_FragColor = vec4(out_value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uint out_value = uint(i % j);"));
    assert!(!source.contains("uint out_value = uint(fmod(i, j));"));
}

#[test]
fn control_flow_coercion_policy_preserves_uint_constructor_compound_int_modulo_operand() {
    let source = concat!(
        "void main() {\n",
        "    int i = 5;\n",
        "    int j = 2;\n",
        "    uint x = uint((i + 1) % j);\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uint x = uint((i + 1) % j);"));
    assert!(!source.contains("uint x = uint(fmod((i + 1), j));"));
}

#[test]
fn control_flow_coercion_policy_preserves_uint_constructor_compound_uint_modulo_operand() {
    let source = concat!(
        "void main() {\n",
        "    uint u = 5u;\n",
        "    uint v = 2u;\n",
        "    uint y = uint((u * 2u + 1u) % v);\n",
        "    gl_FragColor = vec4(y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uint y = uint((u * 2u + 1u) % v);"));
    assert!(!source.contains("uint y = uint(fmod((u * 2u + 1u), v));"));
}

#[test]
fn control_flow_coercion_policy_preserves_audio_bar_unsigned_modulo_forms() {
    let source = concat!(
        "#define RESOLUTION 64\n",
        "void main() {\n",
        "    uint frequency = 5u;\n",
        "    uint barFreq1 = frequency % 16u;\n",
        "    uint barFreq2 = (barFreq1 + 1u) % 16u;\n",
        "    uint barFreq3 = frequency % uint(RESOLUTION);\n",
        "    uint barFreq4 = (barFreq1 + 1u) % uint(RESOLUTION);\n",
        "    gl_FragColor = vec4(float(barFreq1 + barFreq2 + barFreq3 + barFreq4));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uint barFreq1 = frequency % 16u;"));
    assert!(source.contains("uint barFreq2 = (barFreq1 + 1u) % 16u;"));
    assert!(source.contains("uint barFreq3 = frequency % uint(RESOLUTION);"));
    assert!(source.contains("uint barFreq4 = (barFreq1 + 1u) % uint(RESOLUTION);"));
    assert!(!source.contains("uint(fmod("));
    assert!(!source.contains("fmod(frequency"));
}

#[test]
fn control_flow_coercion_policy_lowers_audio_bar_float_modulo_uint_initializers() {
    let source = concat!(
        "#define RESOLUTION 64\n",
        "uniform float u_AudioSpectrumLeft[64];\n",
        "void main() {\n",
        "    float frequency = 5.5;\n",
        "    uint barFreq1 = frequency % RESOLUTION;\n",
        "    uint barFreq2 = (barFreq1 + 1u) % 16u;\n",
        "    float barVolume1 = u_AudioSpectrumLeft[barFreq1];\n",
        "    gl_FragColor = vec4(barVolume1 + float(barFreq2));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "uint barFreq1 = uint(((frequency) - (float(RESOLUTION)) * trunc((frequency) / \
         (float(RESOLUTION)))));"
    ));
    assert!(source.contains("uint barFreq2 = (barFreq1 + 1u) % 16u;"));
    assert!(!source.contains("uint barFreq1 = frequency % RESOLUTION;"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("audio bar float modulo uint initializer should compile through Naga");
}

#[test]
fn control_flow_coercion_policy_integer_modulo_declaration_preserves_nested_texture_sampling() {
    let source = concat!(
        "#define RESOLUTION 64\n",
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    uint barFreq = texSample2D(g_Texture0, v_TexCoord.xy).x % RESOLUTION;\n",
        "    gl_FragColor = vec4(float(barFreq));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "uint barFreq = uint(((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
             v_TexCoord.xy).x) - (float(RESOLUTION)) * trunc((texture(sampler2D(g_Texture0, \
             _we_Sampler_g_Texture0), v_TexCoord.xy).x) / (float(RESOLUTION)))));"
        ),
        "{source}"
    );
    assert!(!source.contains("texSample2D("));
}

#[test]
fn control_flow_coercion_policy_repairs_int_step_initializers() {
    let source = concat!(
        "void main() {\n",
        "    float a = 0.75;\n",
        "    int edge = step(0.5, a);\n",
        "    gl_FragColor = vec4(edge);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float edge = step(0.5, a);"));
    assert!(!source.contains("int edge = step(0.5, a);"));
}

#[test]
fn control_flow_coercion_policy_repairs_qualified_int_step_initializers() {
    let source = concat!(
        "void main() {\n",
        "    const int edge = step(0.5, 0.75);\n",
        "    gl_FragColor = vec4(edge);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("const float edge = step(0.5, 0.75);"));
    assert!(!source.contains("float int edge"));
    assert!(!source.contains("const int edge = step"));
}

#[test]
fn control_flow_coercion_policy_repairs_comment_separated_int_step_initializers() {
    let source = concat!(
        "void main() {\n",
        "    const /*qualifier*/ int /*name*/ edge = step(0.5, 0.75);\n",
        "    gl_FragColor = vec4(edge);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("const /*qualifier*/ float /*name*/ edge = step(0.5, 0.75);"));
    assert!(!source.contains("const /*qualifier*/ int /*name*/ edge = step"));
}

#[test]
fn control_flow_coercion_policy_repairs_int_step_in_later_comma_declarator() {
    let source = concat!(
        "void main() {\n",
        "    float a = 0.75;\n",
        "    int untouched = 1, edge = step(0.5, a);\n",
        "    gl_FragColor = vec4(edge + untouched);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int untouched = 1;"));
    assert!(source.contains("float edge = step(0.5, a);"));
    assert!(!source.contains("float untouched = 1"));
    assert!(!source.contains("int untouched = 1, edge = step(0.5, a);"));
}

#[test]
fn control_flow_coercion_policy_preserves_qualifiers_when_splitting_int_step_declarators() {
    let source = concat!(
        "void main() {\n",
        "    const int keep = 1, edge = step(0.5, 0.75);\n",
        "    gl_FragColor = vec4(edge + keep);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("const int keep = 1;"));
    assert!(source.contains("const float edge = step(0.5, 0.75);"));
    assert!(!source.contains("float int"));
    assert!(!source.contains("\n    float edge = step(0.5, 0.75);"));
}

#[test]
fn control_flow_coercion_policy_composes_int_step_split_with_argument_literal_promotion() {
    let source = concat!(
        "void main() {\n",
        "    float a = 0.75;\n",
        "    int keep = 1, edge = step(0, a);\n",
        "    gl_FragColor = vec4(edge + keep);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int keep = 1;"));
    assert!(source.contains("float edge = step(0.0, a);"));
    assert!(!source.contains("float edge = step(0, a);"));
    assert!(!source.contains("int keep = 1, edge = step(0"));
}

#[test]
fn control_flow_coercion_policy_repairs_int_float_initializers() {
    let source = concat!(
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    int index = floor(v_TexCoord.x * 32);\n",
        "    gl_FragColor = vec4(float(index));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int index = int(floor(v_TexCoord.x * 32));"));
    assert!(!source.contains("int index = floor"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("int initializer from floor should compile after repair");
}

#[test]
fn control_flow_coercion_policy_repairs_int_float_initializers_in_later_comma_declarator() {
    let source = concat!(
        "void main() {\n",
        "    float mixFactor = 0.5;\n",
        "    int keep = 1, index = ceil(mixFactor);\n",
        "    gl_FragColor = vec4(float(index + keep));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int keep = 1, index = int(ceil(mixFactor));"));
    assert!(!source.contains("index = ceil(mixFactor);"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("split int initializer from ceil should compile after repair");
}

#[test]
fn control_flow_coercion_policy_repairs_int_float_swizzle_initializer() {
    let source = concat!(
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    int index = v_TexCoord.x;\n",
        "    gl_FragColor = vec4(float(index));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("int index = int(v_TexCoord.x);"),
        "{source}"
    );
    assert!(!source.contains("int index = v_TexCoord.x;"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("int initializer from vector swizzle should compile after repair");
}

#[test]
fn control_flow_coercion_policy_repairs_int_texture_component_initializer() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    int index = texture2D(g_Texture0, vec2(0.5)).r;\n",
        "    gl_FragColor = vec4(float(index));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "int index = int(texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), vec2(0.5)).r);"
        ),
        "{source}"
    );
    assert!(!source.contains("int index = texture"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("int initializer from texture component should compile after repair");
}

#[test]
fn control_flow_coercion_policy_preserves_int_overloaded_function_initializers() {
    let source = concat!(
        "void main() {\n",
        "    int i = 2;\n",
        "    int lower = 0;\n",
        "    int upper = 3;\n",
        "    int x = clamp(i, lower, upper);\n",
        "    gl_FragColor = vec4(float(x));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("int x = clamp(i, lower, upper);"),
        "{source}"
    );
    assert!(!source.contains("int x = int(clamp"));
}

#[test]
fn control_flow_coercion_policy_preserves_int_abs_initializer() {
    let source = concat!(
        "void main() {\n",
        "    int i = -2;\n",
        "    int x = abs(i);\n",
        "    gl_FragColor = vec4(float(x));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int x = abs(i);"), "{source}");
    assert!(!source.contains("int x = int(abs(i));"));
}

#[test]
fn control_flow_coercion_policy_preserves_int_member_selection_initializers() {
    let source = concat!(
        "struct State { int count; };\n",
        "uniform State s;\n",
        "void main() {\n",
        "    int x = s.count;\n",
        "    gl_FragColor = vec4(float(x));\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("int x = s.count;"), "{source}");
    assert!(!source.contains("int x = int(s.count);"));
}

#[test]
fn control_flow_coercion_policy_lowers_bool_float_initializers() {
    let source = concat!(
        "void main() {\n",
        "    bool condition = true;\n",
        "    float x = condition;\n",
        "    float y = x > 0.5;\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = ((condition) ? 1.0 : 0.0);"));
    assert!(source.contains("float y = ((x > 0.5) ? 1.0 : 0.0);"));
}

#[test]
fn control_flow_coercion_policy_bool_float_initializer_preserves_nested_texture_sampling() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_TexCoord;\n",
        "void main() {\n",
        "    float mask = texSample2D(g_Texture0, v_TexCoord.xy).x > 0.5;\n",
        "    gl_FragColor = vec4(mask);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "float mask = ((texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord.xy).x > \
         0.5) ? 1.0 : 0.0);"
    ));
    assert!(!source.contains("texSample2D("));
}

#[test]
fn control_flow_coercion_policy_float_times_bool_preserves_nested_reserved_identifier() {
    let source = concat!(
        "void main() {\n",
        "    bool sample = true;\n",
        "    float f = 1.0;\n",
        "    f *= sample;\n",
        "    gl_FragColor = vec4(f);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("bool sample_local = true;"));
    assert!(
        source.contains("f *= (sample_local ? 1.0 : 0.0);"),
        "{source}"
    );
    assert!(!source.contains("f *= (sample ? 1.0 : 0.0);"));
    let _artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("float-times-bool coercion should preserve reserved identifier fixup");
}

#[test]
fn control_flow_coercion_policy_lowers_bool_float_in_later_comma_declarator() {
    let source = concat!(
        "void main() {\n",
        "    bool condition = true;\n",
        "    float x = 0.0, y = condition;\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = 0.0, y = ((condition) ? 1.0 : 0.0);"));
    assert!(!source.contains("float x = ((0.0, y = condition) ? 1.0 : 0.0);"));
}

#[test]
fn control_flow_coercion_policy_lowers_multiple_bool_float_comma_declarators() {
    let source = concat!(
        "void main() {\n",
        "    bool condition = true;\n",
        "    float x = condition, y = condition;\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = ((condition) ? 1.0 : 0.0), y = ((condition) ? 1.0 : 0.0);"));
    assert!(!source.contains("y = condition"));
}

#[test]
fn control_flow_coercion_policy_does_not_guess_bool_from_identifier_name() {
    let source = concat!(
        "void main() {\n",
        "    float brightness = 0.8;\n",
        "    float value = brightness;\n",
        "    value *= brightness;\n",
        "    gl_FragColor = vec4(value);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float value = brightness;"));
    assert!(source.contains("value *= brightness;"));
    assert!(!source.contains("brightness) ? 1.0 : 0.0"));
}

#[test]
fn control_flow_coercion_policy_lowers_float_times_bool_assignments() {
    let source = concat!(
        "void main() {\n",
        "    bool enabled = true;\n",
        "    float intensity = 1.0;\n",
        "    intensity *= enabled;\n",
        "    gl_FragColor = vec4(intensity);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("intensity *= (enabled ? 1.0 : 0.0);"));
}

#[test]
fn alpha_to_coverage_derivative_idiom_reuses_pre_derivative_color_alpha() {
    let source = concat!(
        "void main() {\n",
        "    vec4 color = vec4(0.25, 0.5, 0.75, 0.6);\n",
        "    gl_FragColor = color;\n",
        "    gl_FragColor.a = saturate((gl_FragColor.a - 0.5) / max(fwidth(gl_FragColor.a), \
         0.0001) + 0.5);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("_we_FragColor.a = clamp(color.a, 0.0, 1.0);"));
    assert!(!source.contains("fwidth(_we_FragColor.a)"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("alpha-to-coverage derivative idiom should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn alpha_to_coverage_derivative_idiom_tracks_non_color_fragment_assignment_source() {
    let source = concat!(
        "void main() {\n",
        "    vec4 sampled = vec4(0.25, 0.5, 0.75, 0.6);\n",
        "    gl_FragColor = sampled;\n",
        "    gl_FragColor.a = saturate((gl_FragColor.a - 0.5) / max(fwidth(gl_FragColor.a), \
         0.0001) + 0.5);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("_we_FragColor.a = clamp(sampled.a, 0.0, 1.0);"));
    assert!(!source.contains("color.a"));
    assert!(!source.contains("fwidth(_we_FragColor.a)"));
}

#[test]
fn alpha_to_coverage_derivative_idiom_preserves_nested_reserved_source_identifier() {
    let source = concat!(
        "void main() {\n",
        "    vec4 sample = vec4(0.25, 0.5, 0.75, 0.6);\n",
        "    gl_FragColor = sample;\n",
        "    gl_FragColor.a = saturate((gl_FragColor.a - 0.5) / max(fwidth(gl_FragColor.a), \
         0.0001) + 0.5);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec4 sample_local = vec4(0.25, 0.5, 0.75, 0.6);"));
    assert!(source.contains("_we_FragColor = sample_local;"));
    assert!(source.contains("_we_FragColor.a = clamp(sample_local.a, 0.0, 1.0);"));
    assert!(!source.contains("_we_FragColor.a = clamp(sample.a"));
    assert!(!source.contains("fwidth(_we_FragColor.a)"));
    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect(
            "alpha-to-coverage derivative idiom should preserve nested source identifier fixups",
        );
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn alpha_to_coverage_derivative_idiom_ignores_out_of_scope_color_declarations() {
    let source = concat!(
        "void main() {\n",
        "    {\n",
        "        vec4 color = vec4(0.1);\n",
        "    }\n",
        "    vec4 sampled = vec4(0.25, 0.5, 0.75, 0.6);\n",
        "    gl_FragColor = sampled;\n",
        "    gl_FragColor.a = saturate((gl_FragColor.a - 0.5) / max(fwidth(gl_FragColor.a), \
         0.0001) + 0.5);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("_we_FragColor.a = clamp(sampled.a, 0.0, 1.0);"));
    assert!(!source.contains("_we_FragColor.a = clamp(color.a"));
    assert!(!source.contains("fwidth(_we_FragColor.a)"));
}

#[test]
fn alpha_to_coverage_derivative_idiom_does_not_reuse_same_named_helper_binding() {
    let source = concat!(
        "void helper() {\n",
        "    vec4 color = vec4(0.1);\n",
        "    gl_FragColor = color;\n",
        "}\n",
        "void main() {\n",
        "    vec4 color = vec4(0.9);\n",
        "    gl_FragColor.a = saturate((gl_FragColor.a - 0.5) / max(fwidth(gl_FragColor.a), \
         0.0001) + 0.5);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(!source.contains("_we_FragColor.a = clamp(color.a, 0.0, 1.0);"));
    assert!(source.contains("fwidth(_we_FragColor.a)"));
}

#[test]
fn texture_sampling_policy_handles_implicit_and_explicit_lod() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    vec2 uv = vec2(0.5);\n",
        "    vec4 a = tex2D(g_Texture0, uv);\n",
        "    vec4 b = texture2D(g_Texture0, uv);\n",
        "    vec4 c = texSample2D(g_Texture0, uv);\n",
        "    vec4 d = textureLod(g_Texture0, uv, 1.0);\n",
        "    vec4 e = texSample2DLod(g_Texture0, uv, 2.0);\n",
        "    gl_FragColor = a + b + c + d + e;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec4 a = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv);")
    );
    assert!(
        source.contains("vec4 b = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv);")
    );
    assert!(
        source.contains("vec4 c = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv);")
    );
    assert!(
        source.contains(
            "vec4 d = textureLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv, 1.0);"
        )
    );
    assert!(
        source.contains(
            "vec4 e = textureLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv, 2.0);"
        )
    );
}

#[test]
fn reserved_identifier_policy_renames_user_symbols_only() {
    let source = concat!(
        "float mod(float x) { return x; }\n",
        "float sample(float x) { return x + 1.0; }\n",
        "void main() {\n",
        "    vec2 wrapped = mod(vec2(5.5), vec2(2.0));\n",
        "    float user_mod = mod(1.0);\n",
        "    float user_sample = sample(2.0);\n",
        "    gl_FragColor = vec4(wrapped, user_mod + user_sample, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float _we_user_mod(float x)"));
    assert!(source.contains("float _we_user_sample(float x)"));
    assert!(source.contains("vec2 wrapped = mod(vec2(5.5), vec2(2.0));"));
    assert!(source.contains("float user_mod = _we_user_mod(1.0);"));
    assert!(source.contains("float user_sample = _we_user_sample(2.0);"));
}

#[test]
fn reserved_identifier_policy_respects_nested_shadowing_scope() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    float uv = 1.0;\n",
        "    float outer = uv;\n",
        "    {\n",
        "        float uv = 2.0;\n",
        "        outer += uv;\n",
        "    }\n",
        "    outer += uv;\n",
        "    gl_FragColor = vec4(outer);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float uv_local = 1.0;"));
    assert!(source.contains("float outer = uv_local;"));
    assert!(source.contains("        float uv_local_1 = 2.0;"));
    assert!(source.contains("        outer += uv_local_1;"));
    assert!(source.contains("    outer += uv_local;"));
    assert!(!source.contains("        outer += uv;"));
}

#[test]
fn reserved_identifier_policy_respects_uninitialized_nested_shadowing_scope() {
    let source = concat!(
        "varying vec2 uv;\n",
        "void main() {\n",
        "    float uv = 1.0;\n",
        "    float outer = uv;\n",
        "    {\n",
        "        float uv;\n",
        "        uv = 2.0;\n",
        "        outer += uv;\n",
        "    }\n",
        "    outer += uv;\n",
        "    gl_FragColor = vec4(outer);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float uv_local = 1.0;"));
    assert!(source.contains("float outer = uv_local;"));
    assert!(source.contains("        float uv_local_1;"));
    assert!(source.contains("        uv_local_1 = 2.0;"));
    assert!(source.contains("        outer += uv_local_1;"));
    assert!(source.contains("    outer += uv_local;"));
    assert!(!source.contains("        uv_local = 2.0;"));
    assert!(!source.contains("        outer += uv_local;"));
}
