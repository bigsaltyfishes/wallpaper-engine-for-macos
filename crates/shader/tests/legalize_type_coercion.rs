use shader::{
    ShaderCompiler, ShaderStageKind,
    compile::NagaCompiler,
    legalize::{Codegen, CodegenStageSource},
    syntax::ShaderModule,
};

fn legalize(stage: ShaderStageKind, source: &str) -> CodegenStageSource {
    let module = ShaderModule::parse(stage, source).expect("module parses");
    Codegen.legalize(&module).expect("shader legalizes")
}

#[test]
fn type_coercion_strategy_widens_vec2_constructor_in_vec3_binary_expression() {
    let source = concat!(
        "void main() {\n",
        "    vec3 base = vec3(1.0);\n",
        "    vec3 shifted = base + vec2(0.25, 0.5);\n",
        "    vec3 lowered = base - CAST2(0.25);\n",
        "    gl_FragColor = vec4(shifted + lowered, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 shifted = base + vec3(vec2(0.25, 0.5), 0.0);"));
    assert!(source.contains("vec3 lowered = base - vec3(vec2(0.25), 0.0);"));
}

#[test]
fn type_coercion_strategy_does_not_widen_vec2_constructor_next_to_swizzled_vec3() {
    let source = concat!(
        "void main() {\n",
        "    vec3 offset = vec3(0.001, 0.002, 0.003);\n",
        "    vec4 coords = vec4(0.0);\n",
        "    float chromatic = 0.5;\n",
        "    coords.xz += offset.xy + vec2(0.005, -0.0005) * chromatic;\n",
        "    gl_FragColor = coords;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("coords.xz += offset.xy + vec2(0.005, -0.0005) * chromatic;"),
        "{source}"
    );
    assert!(!source.contains("vec3(vec2(0.005, -0.0005), 0.0)"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("swizzled vec3 peer should keep vec2 expression width");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_does_not_widen_vec2_initializer_binary_expression() {
    let source = concat!(
        "vec2 rotateVec2(vec2 v, float r) {\n",
        "    vec2 cs = vec2(cos(r), sin(r));\n",
        "    return vec2(v.x * cs.x - v.y * cs.y, v.x * cs.y + v.y * cs.x);\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(rotateVec2(vec2(0.0, 1.0), 0.5), 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec2 cs = vec2(cos(r), sin(r));"),
        "{source}"
    );
    assert!(!source.contains("vec2 cs = vec3(vec2(cos(r), sin(r)), 0.0);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("rotateVec2 helper should preserve vec2 initializer width");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_does_not_widen_common_header_rotate_vec2_helper() {
    let source = concat!(
        "vec3 hsv2rgb(vec3 c) {\n",
        "    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);\n",
        "    vec3 p = abs(frac(c.xxx + K.xyz) * 6.0 - K.www);\n",
        "    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);\n",
        "}\n",
        "vec2 rotateVec2(vec2 v, float r) {\n",
        "    vec2 cs = vec2(cos(r), sin(r));\n",
        "    return vec2(v.x * cs.x - v.y * cs.y, v.x * cs.y + v.y * cs.x);\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(rotateVec2(vec2(0.0, 1.0), 0.5), 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec2 cs = vec2(cos(r), sin(r));"),
        "{source}"
    );
    assert!(!source.contains("vec2 cs = vec3(vec2(cos(r), sin(r)), 0.0);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("common.h rotateVec2 helper should preserve vec2 initializer width");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_narrows_vec4_constructor_for_vec3_initializer() {
    let source = concat!(
        "void main() {\n",
        "    vec4 r = vec4(1.0, 0.0, 0.0, 1.0);\n",
        "    vec4 g = vec4(0.0, 1.0, 0.0, 1.0);\n",
        "    vec4 b = vec4(0.0, 0.0, 1.0, 1.0);\n",
        "    vec3 finalColor = vec4(r.r, g.g, b.b, 0.1);\n",
        "    gl_FragColor = vec4(finalColor, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 finalColor = (vec4(r.r, g.g, b.b, 0.1)).xyz;"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vec4 constructor initializer should narrow for vec3 declaration");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_broadcasts_shadowed_scalar_identifier() {
    let source = concat!(
        "void main() {\n",
        "    vec2 amount = vec2(0.25);\n",
        "    vec2 color = vec2(0.0);\n",
        "    if (true) {\n",
        "        float amount = 0.5;\n",
        "        color = max(amount, vec2(1.0));\n",
        "    }\n",
        "    gl_FragColor = vec4(color, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color = max(vec2(amount_local), vec2(1.0));"));
    assert!(!source.contains("color = max(amount_local, vec2(1.0));"));
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_expression_in_vector_max() {
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
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_literal_before_swizzled_vector_max() {
    let source = concat!(
        "void main() {\n",
        "    vec4 albedo = vec4(0.25, 0.5, 0.75, 1.0);\n",
        "    gl_FragColor = vec4(max(0, albedo.rgb), albedo.a);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("max(vec3(0.0), albedo.rgb)"));
    assert!(!source.contains("max(0, albedo.rgb)"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vector max with swizzled vector operand should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_literal_before_vector_call_max() {
    let source = concat!(
        "void main() {\n",
        "    vec2 scale = vec2(1.25);\n",
        "    vec2 factor = max(1, abs(scale));\n",
        "    gl_FragColor = vec4(factor, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec2 factor = max(vec2(1.0), abs(scale));"),
        "{source}"
    );
    assert!(!source.contains("max(1, abs(scale))"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vector max with vector-returning peer should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_literal_before_uniform_vector_call_max() {
    let source = concat!(
        "uniform vec2 u_ShadowScale;\n",
        "void main() {\n",
        "    vec2 factor = max(1, abs(u_ShadowScale));\n",
        "    gl_FragColor = vec4(factor, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("vec2 factor = max(vec2(1.0), abs(u_ShadowScale));"),
        "{source}"
    );
    assert!(!source.contains("max(1, abs(u_ShadowScale))"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vector max with top-level vector uniform should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_repairs_shadow_antitruncation_factor_initializer() {
    let source = concat!(
        "uniform vec2 u_shadowOffset;\n",
        "uniform vec2 g_ParallaxPosition;\n",
        "uniform vec2 u_ParallaxScale;\n",
        "uniform vec2 u_ShadowScale;\n",
        "void main() {\n",
        "    float atFactor = (1 + (abs(u_shadowOffset) + abs(g_ParallaxPosition * \
         u_ParallaxScale)) * 2) * max(1, abs(u_ShadowScale));\n",
        "    gl_Position = vec4(vec2(atFactor), 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Vertex, source);
    let source = legalized.source();

    assert!(!source.contains("max(1, abs(u_ShadowScale))"), "{source}");

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &legalized)
        .expect("shadow anti-truncation initializer should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_literal_before_swizzled_texture_sample() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec2 v_Uv;\n",
        "void main() {\n",
        "    vec3 color = max(0.5, texSample2D(g_Texture0, v_Uv).rgb);\n",
        "    gl_FragColor = vec4(color, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec3 color = max(vec3(0.5), texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
         v_Uv).rgb);"
    ));
    assert!(!source.contains("max(0.5, texSample2D("));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("swizzled texture sample max coercion should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_swizzles_nested_binary_operand_in_vec2_initializer() {
    let source = concat!(
        "void main() {\n",
        "    vec2 uv = vec2(0.25, 0.5);\n",
        "    vec3 normal = vec3(0.0, 0.0, 1.0);\n",
        "    float scale = 0.5;\n",
        "    vec2 out_uv = uv + normal * scale;\n",
        "    gl_FragColor = vec4(out_uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec2 out_uv = uv + normal.xy * scale;"));
    assert!(!source.contains("vec2 out_uv = uv + normal * scale;"));
}

#[test]
fn type_coercion_strategy_swizzles_nested_binary_operand_in_vec2_assignment() {
    let source = concat!(
        "void main() {\n",
        "    vec2 uv = vec2(0.25, 0.5);\n",
        "    vec3 normal = vec3(0.0, 0.0, 1.0);\n",
        "    float scale = 0.5;\n",
        "    vec2 out_uv = vec2(0.0);\n",
        "    out_uv = uv + normal * scale;\n",
        "    gl_FragColor = vec4(out_uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("out_uv = uv + normal.xy * scale;"));
    assert!(!source.contains("out_uv = uv + normal * scale;"));
}

#[test]
fn type_coercion_strategy_uses_nested_vector_call_width_for_max() {
    let source = concat!(
        "void main() {\n",
        "    vec2 scale = vec2(1.0, 1.25);\n",
        "    float factor = max(1, abs(scale)).x;\n",
        "    gl_FragColor = vec4(factor);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float factor = max(vec2(1.0), abs(scale)).x;"));
    assert!(!source.contains("float factor = max(1, abs(scale)).x;"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("nested vector-returning max argument should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_treats_void_signature_as_zero_argument_vector_return() {
    let source = concat!(
        "vec2 amount(void) {\n",
        "    return vec2(0.25, 0.5);\n",
        "}\n",
        "void main() {\n",
        "    float factor = amount();\n",
        "    gl_FragColor = vec4(factor);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float factor = amount().x;"), "{source}");
    assert!(!source.contains("float factor = amount();"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar initializer from void-signature vector return should compile");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_vec4_coordinates_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, v_TexCoord);\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord.xy);"
    ));
    assert!(!source.contains("sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vec4 coordinate should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_vector_expression_coordinates_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 timer = vec4(0.1);\n",
        "    vec4 scene = texSample2D(g_Texture0, v_TexCoord.xy - timer);\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord.xy - \
         timer.xy);"
    ));
    assert!(
        !source.contains("sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord.xy - timer);")
    );

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vector expression coordinate should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_each_vec4_coordinate_operand_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "varying vec4 v_TexOffset;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, v_TexCoord + v_TexOffset);\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), v_TexCoord.xy + \
         v_TexOffset.xy);"
    ));
    assert!(!source.contains("v_TexCoord.xy + v_TexOffset);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vector coordinate operands should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_parenthesized_vec4_coordinate_operands_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "varying vec4 v_TexOffset;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, (v_TexCoord + v_TexOffset));\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), (v_TexCoord.xy + \
         v_TexOffset.xy));"
    ));
    assert!(!source.contains("(v_TexCoord + v_TexOffset)"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect(
            "sampler2D parenthesized vector coordinate operands should compile after narrowing",
        );
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_wraps_vec4_call_coordinate_operand_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "varying vec2 v_TexOffset;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, abs(v_TexCoord) + v_TexOffset);\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), (abs(v_TexCoord)).xy \
         + v_TexOffset);"
    ));
    assert!(!source.contains("abs(v_TexCoord).xy + v_TexOffset"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vector call coordinate operand should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_wraps_direct_vec4_call_coordinate_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, abs(v_TexCoord));\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
         (abs(v_TexCoord)).xy);"
    ));
    assert!(!source.contains("abs(v_TexCoord).xy"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D direct vector call coordinate should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_vector_binary_inside_call_coordinate_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "varying vec4 v_TexOffset;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, abs(v_TexCoord + v_TexOffset));\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), (abs(v_TexCoord + \
         v_TexOffset)).xy);"
    ));
    assert!(!source.contains(
        "texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), abs(v_TexCoord + v_TexOffset));"
    ));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vector binary inside call coordinate should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_parenthesized_vector_inside_call_coordinate_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec4 v_TexCoord;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, abs((v_TexCoord)));\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), \
         (abs((v_TexCoord))).xy);"
    ));
    assert!(
        !source
            .contains("texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), abs((v_TexCoord)));")
    );

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect(
            "sampler2D parenthesized vector inside call coordinate should compile after narrowing",
        );
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_selects_scalar_component_from_vector_expression_initializer() {
    let source = concat!(
        "void main() {\n",
        "    vec2 shadow_scale = vec2(1.0, 1.25);\n",
        "    float at_factor = 2.0 * max(1, abs(shadow_scale));\n",
        "    gl_FragColor = vec4(at_factor);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float at_factor = (2.0 * max(vec2(1.0), abs(shadow_scale))).x;"));
    assert!(!source.contains("float at_factor = 2.0 * max(vec2(1.0), abs(shadow_scale));"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar initializer from vector expression should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_keeps_binary_expression_width_after_vector_builtin_call() {
    let source = concat!(
        "void main() {\n",
        "    vec2 scale = vec2(1.0, 1.25);\n",
        "    float factor = max(1, abs(scale)) * (2.0);\n",
        "    gl_FragColor = vec4(factor);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains("float factor = (max(vec2(1.0), abs(scale)) * (2.0)).x;"),
        "{source}"
    );
    assert!(!source.contains("float factor = max(vec2(1.0), abs(scale)) * (2.0);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar initializer from vector call binary expression should compile");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn texture_sampling_strategy_narrows_vec3_coordinates_for_sampler2d() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "varying vec3 uv3;\n",
        "void main() {\n",
        "    vec4 scene = texSample2D(g_Texture0, uv3);\n",
        "    gl_FragColor = scene;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source.contains(
            "vec4 scene = texture(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv3.xy);"
        )
    );
    assert!(!source.contains("sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv3);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("sampler2D vec3 coordinate should compile after narrowing");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_uses_overload_signature_for_scalar_call_assignment() {
    let source = concat!(
        "float choose(float value) {\n",
        "    return value;\n",
        "}\n",
        "vec4 choose(vec4 value) {\n",
        "    return value;\n",
        "}\n",
        "void main() {\n",
        "    vec4 color = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    color += choose(time);\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color += vec4(choose(time));"), "{source}");
    assert!(!source.contains("color += choose(time);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar overload call compound assignment should compile after wrapping");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_avoids_wrapping_conflicting_overload_return_evidence() {
    let source = concat!(
        "float choose(float value) {\n",
        "    return value;\n",
        "}\n",
        "vec4 choose(float value) {\n",
        "    return vec4(value);\n",
        "}\n",
        "void main() {\n",
        "    vec4 color = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    color += choose(time);\n",
        "    gl_FragColor = color;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color += choose(time);"), "{source}");
    assert!(!source.contains("color += vec4(choose(time));"));
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "void main() {\n",
        "    vec4 uv = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    uv += time * 0.5;\n",
        "    gl_FragColor = uv;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uv += vec4(time * 0.5);"));
    assert!(!source.contains("uv += time * 0.5;"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar compound assignment to vector lhs should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_broadcasts_scalar_call_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "float amount(float value) {\n",
        "    return sin(value);\n",
        "}\n",
        "void main() {\n",
        "    vec4 uv = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    uv += amount(time);\n",
        "    gl_FragColor = uv;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uv += vec4(amount(time));"));
    assert!(!source.contains("uv += amount(time);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("scalar call compound assignment to vector lhs should compile through Naga");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_preserves_vector_call_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "vec4 amount(float value) {\n",
        "    return vec4(value);\n",
        "}\n",
        "void main() {\n",
        "    vec4 uv = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    uv += amount(time);\n",
        "    gl_FragColor = uv;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uv += amount(time);"));
    assert!(!source.contains("uv += vec4(amount(time));"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect("vector-returning call compound assignment should compile without scalar wrapping");
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_preserves_vector_call_expression_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "vec4 amount(float value) {\n",
        "    return vec4(value);\n",
        "}\n",
        "void main() {\n",
        "    vec4 uv = vec4(0.25);\n",
        "    float time = 1.0;\n",
        "    uv += amount(time) * 0.5;\n",
        "    gl_FragColor = uv;\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("uv += amount(time) * 0.5;"));
    assert!(!source.contains("uv += vec4(amount(time) * 0.5);"));

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &legalized)
        .expect(
            "vector-returning call expression compound assignment should compile without scalar \
             wrapping",
        );
    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
}

#[test]
fn type_coercion_strategy_preserves_texture_query_lod_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    vec2 uv = vec2(0.25);\n",
        "    uv += textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5;\n",
        "    gl_FragColor = vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "uv += textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5;"
    ));
    assert!(!source.contains(
        "uv += vec2(textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5);"
    ));
}

#[test]
fn type_coercion_strategy_uses_texture_query_levels_as_scalar_evidence_only() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    vec2 uv = vec2(0.25);\n",
        "    uv += textureQueryLevels(sampler2D(g_Texture0, _we_Sampler_g_Texture0)) * 2;\n",
        "    uv += textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5;\n",
        "    uv += textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2;\n",
        "    gl_FragColor = vec4(uv, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains(
        "uv += vec2(textureQueryLevels(sampler2D(g_Texture0, _we_Sampler_g_Texture0)) * 2);"
    ));
    assert!(source.contains(
        "uv += textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5;"
    ));
    assert!(
        source.contains("uv += textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2;")
    );
    assert!(!source.contains(
        "uv += vec2(textureQueryLod(sampler2D(g_Texture0, _we_Sampler_g_Texture0), uv) * 0.5);"
    ));
    assert!(!source.contains(
        "uv += vec2(textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2);"
    ));
}

#[test]
fn type_coercion_strategy_preserves_texture_size_expression_compound_assignment_to_vector_lhs() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "void main() {\n",
        "    ivec2 size = ivec2(0);\n",
        "    size += textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2;\n",
        "    gl_FragColor = vec4(vec2(size), 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(
        source
            .contains("size += textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2;")
    );
    assert!(!source.contains(
        "size += vec2(textureSize(sampler2D(g_Texture0, _we_Sampler_g_Texture0), 0) / 2);"
    ));
}

#[test]
fn type_coercion_strategy_uses_nearest_vector_width_for_vec3_vec2_widening() {
    let source = concat!(
        "void main() {\n",
        "    vec3 base = vec3(1.0);\n",
        "    vec3 widened = base + vec2(0.25, 0.5);\n",
        "    if (true) {\n",
        "        vec2 base = vec2(0.0);\n",
        "        vec2 unchanged = base + vec2(0.25, 0.5);\n",
        "        widened += vec3(unchanged, 0.0);\n",
        "    }\n",
        "    gl_FragColor = vec4(widened, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 widened = base + vec3(vec2(0.25, 0.5), 0.0);"));
    assert!(source.contains("vec2 unchanged = base_local + vec2(0.25, 0.5);"));
    assert!(!source.contains("vec2 unchanged = base_local + vec3(vec2(0.25, 0.5), 0.0);"));
}

#[test]
fn type_coercion_strategy_stops_vec3_vec2_widening_at_aggregate_local_blocker() {
    let source = concat!(
        "void main() {\n",
        "    vec3 base = vec3(1.0);\n",
        "    vec3 widened = base + vec2(0.25, 0.5);\n",
        "    if (true) {\n",
        "        ivec3 base = ivec3(1);\n",
        "        vec3 outv = base + vec2(1.0);\n",
        "        widened += vec3(float(outv.x));\n",
        "    }\n",
        "    gl_FragColor = vec4(widened, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 widened = base + vec3(vec2(0.25, 0.5), 0.0);"));
    assert!(source.contains("vec3 outv = base_local + vec2(1.0);"));
    assert!(!source.contains("vec3 outv = base_local + vec3(vec2(1.0), 0.0);"));
}

#[test]
fn type_coercion_strategy_stops_builtin_scalar_broadcast_at_aggregate_local_blocker() {
    let source = concat!(
        "void main() {\n",
        "    vec2 amount = vec2(0.25);\n",
        "    vec2 color = vec2(0.0);\n",
        "    if (true) {\n",
        "        ivec2 amount = ivec2(1);\n",
        "        color = max(amount, vec2(1.0));\n",
        "    }\n",
        "    gl_FragColor = vec4(color, 0.0, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("color = max(amount_local, vec2(1.0));"));
    assert!(!source.contains("color = max(vec2(amount_local), vec2(1.0));"));
}

#[test]
fn type_coercion_strategy_stops_vec3_vec2_widening_at_aggregate_parameter_blocker() {
    let source = concat!(
        "vec3 base = vec3(1.0);\n",
        "vec3 helper(ivec3 base) {\n",
        "    return base + vec2(1.0);\n",
        "}\n",
        "void main() {\n",
        "    vec3 widened = base + vec2(0.25, 0.5);\n",
        "    gl_FragColor = vec4(widened + helper(ivec3(1)), 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 widened = base + vec3(vec2(0.25, 0.5), 0.0);"));
    assert!(source.contains("return base + vec2(1.0);"));
    assert!(!source.contains("return base + vec3(vec2(1.0), 0.0);"));
}

#[test]
fn type_coercion_strategy_tracks_common_aggregate_blocker_spellings() {
    let source = concat!(
        "void main() {\n",
        "    vec3 u_shadow = vec3(1.0);\n",
        "    vec3 b_shadow = vec3(1.0);\n",
        "    vec3 m2_shadow = vec3(1.0);\n",
        "    vec3 m3_shadow = vec3(1.0);\n",
        "    vec3 m4_shadow = vec3(1.0);\n",
        "    if (true) {\n",
        "        uvec3 u_shadow = uvec3(1u);\n",
        "        bvec3 b_shadow = bvec3(true);\n",
        "        mat2 m2_shadow = mat2(1.0);\n",
        "        mat3 m3_shadow = mat3(1.0);\n",
        "        mat4 m4_shadow = mat4(1.0);\n",
        "        vec3 a = u_shadow + vec2(1.0);\n",
        "        vec3 b = b_shadow + vec2(1.0);\n",
        "        vec3 c = m2_shadow + vec2(1.0);\n",
        "        vec3 d = m3_shadow + vec2(1.0);\n",
        "        vec3 e = m4_shadow + vec2(1.0);\n",
        "    }\n",
        "    gl_FragColor = vec4(u_shadow + b_shadow + m2_shadow + m3_shadow + m4_shadow, 1.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("vec3 a = u_shadow_local + vec2(1.0);"));
    assert!(source.contains("vec3 b = b_shadow_local + vec2(1.0);"));
    assert!(source.contains("vec3 c = m2_shadow_local + vec2(1.0);"));
    assert!(source.contains("vec3 d = m3_shadow_local + vec2(1.0);"));
    assert!(source.contains("vec3 e = m4_shadow_local + vec2(1.0);"));
    assert!(!source.contains("_shadow_local + vec3(vec2(1.0), 0.0);"));
}

#[test]
fn control_flow_strategy_casts_integer_for_loop_bounds() {
    let source = concat!(
        "#define RESOLUTION 64\n",
        "void main() {\n",
        "    float begin = 0.0;\n",
        "    for (int i = begin; i < RESOLUTION; i++) {\n",
        "        gl_FragColor = vec4(float(i));\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (int i = int(begin); i < int(RESOLUTION); i++) {"));
}

#[test]
fn control_flow_strategy_casts_integer_for_loop_inclusive_bounds() {
    let source = concat!(
        "#define MIN_RESOLUTION 0\n",
        "#define MAX_RESOLUTION 64\n",
        "void main() {\n",
        "    float begin = 0.0;\n",
        "    for (int i = begin; i <= MAX_RESOLUTION; i++) {\n",
        "        gl_FragColor = vec4(float(i));\n",
        "    }\n",
        "    for (int j = begin; j >= MIN_RESOLUTION; j--) {\n",
        "        gl_FragColor += vec4(float(j));\n",
        "    }\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (int i = int(begin); i <= int(MAX_RESOLUTION); i++) {"));
    assert!(source.contains("for (int j = int(begin); j >= int(MIN_RESOLUTION); j--) {"));
    assert!(!source.contains("<= int(= MAX_RESOLUTION)"));
    assert!(!source.contains(">= int(= MIN_RESOLUTION)"));
}

#[test]
fn control_flow_strategy_does_not_cast_float_for_loop_bounds() {
    let source = concat!(
        "void main() {\n",
        "    float sum = 0.0;\n",
        "    for (float t = 0.0; t < 0.5; t += 0.1) {\n",
        "        sum += t;\n",
        "    }\n",
        "    gl_FragColor = vec4(sum);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("for (float t = 0.0; t < 0.5; t += 0.1) {"));
    assert!(!source.contains("t < int(0.5)"));
}

#[test]
fn control_flow_strategy_uses_nearest_binding_for_bool_coercion() {
    let source = concat!(
        "void main() {\n",
        "    bool enabled = true;\n",
        "    float outer = enabled;\n",
        "    {\n",
        "        float enabled = 0.25;\n",
        "        float inner = enabled;\n",
        "        inner *= enabled;\n",
        "        outer += inner;\n",
        "    }\n",
        "    gl_FragColor = vec4(outer);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float outer = ((enabled) ? 1.0 : 0.0);"));
    assert!(source.contains("float enabled_local = 0.25;"));
    assert!(source.contains("float inner = enabled_local;"));
    assert!(source.contains("inner *= enabled_local;"));
    assert!(!source.contains("float inner = ((enabled_local) ? 1.0 : 0.0);"));
    assert!(!source.contains("inner *= (enabled_local ? 1.0 : 0.0);"));
}

#[test]
fn control_flow_strategy_tracks_bool_comma_declarators() {
    let source = concat!(
        "void main() {\n",
        "    bool a = true, b = false;\n",
        "    float x = b;\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = ((b) ? 1.0 : 0.0);"));
    assert!(!source.contains("float x = b;"));
}

#[test]
fn control_flow_strategy_tracks_comment_separated_bool_declarations() {
    let source = concat!(
        "void main() {\n",
        "    bool /*name*/ enabled = true;\n",
        "    float /*name*/ x = enabled;\n",
        "    gl_FragColor = vec4(x);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float /*name*/ x = ((enabled) ? 1.0 : 0.0);"));
    assert!(!source.contains("float /*name*/ x = enabled;"));
}

#[test]
fn control_flow_strategy_ignores_function_prototype_parameters() {
    let source = concat!(
        "float proto_flag;\n",
        "float header_flag;\n",
        "void helper(bool proto_flag);\n",
        "void other(bool header_flag) { }\n",
        "void main() {\n",
        "    float x = proto_flag;\n",
        "    float y = header_flag;\n",
        "    gl_FragColor = vec4(x + y);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = proto_flag;"));
    assert!(source.contains("float y = header_flag;"));
    assert!(!source.contains("float x = ((proto_flag) ? 1.0 : 0.0);"));
    assert!(!source.contains("float y = ((header_flag) ? 1.0 : 0.0);"));
}

#[test]
fn control_flow_strategy_tracks_function_body_bool_parameters() {
    let source = concat!(
        "bool helper(bool enabled) {\n",
        "    float x = enabled;\n",
        "    return enabled;\n",
        "}\n",
        "void main() {\n",
        "    gl_FragColor = vec4(helper(true) ? 1.0 : 0.0);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("float x = ((enabled) ? 1.0 : 0.0);"));
}

#[test]
fn control_flow_strategy_uses_type_appropriate_numeric_condition_zero_literals() {
    let source = concat!(
        "void main() {\n",
        "    float f = 1.0;\n",
        "    int i = 1;\n",
        "    uint u = 1u;\n",
        "    if (f) { f += 1.0; }\n",
        "    if (i) { f += 2.0; }\n",
        "    if (u) { f += 3.0; }\n",
        "    if (1) { f += 4.0; }\n",
        "    if (1u) { f += 5.0; }\n",
        "    if (1.0) { f += 6.0; }\n",
        "    gl_FragColor = vec4(f);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("if (f != 0.0)"));
    assert!(source.contains("if (i != 0)"));
    assert!(source.contains("if (u != 0u)"));
    assert!(source.contains("if (1 != 0)"));
    assert!(source.contains("if (1u != 0u)"));
    assert!(source.contains("if (1.0 != 0.0)"));
    assert!(!source.contains("i != 0.0"));
    assert!(!source.contains("u != 0.0"));
    assert!(!source.contains("1u != 0.0"));
}

#[test]
fn control_flow_strategy_uses_unsigned_zero_for_unsigned_arithmetic_conditions() {
    let source = concat!(
        "void main() {\n",
        "    float f = 1.0;\n",
        "    uint u = 3u;\n",
        "    if (u + 1u) { f += 1.0; }\n",
        "    while (u % 2u) { f += 2.0; break; }\n",
        "    if ((u * 2u + 1u)) { f += 3.0; }\n",
        "    gl_FragColor = vec4(f);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("if (u + 1u != 0u)"));
    assert!(source.contains("while (u % 2u != 0u)"));
    assert!(source.contains("if ((u * 2u + 1u) != 0u)"));
    assert!(!source.contains("u + 1u != 0)"));
    assert!(!source.contains("u % 2u != 0)"));
    assert!(!source.contains("u * 2u + 1u) != 0)"));
}

#[test]
fn control_flow_strategy_leaves_mixed_signedness_arithmetic_conditions_unrewritten() {
    let source = concat!(
        "void main() {\n",
        "    float f = 1.0;\n",
        "    int i = 2;\n",
        "    uint u = 3u;\n",
        "    if (u + i) { f += 1.0; }\n",
        "    while (u % i) { f += 2.0; break; }\n",
        "    gl_FragColor = vec4(f);\n",
        "}\n",
    );

    let legalized = legalize(ShaderStageKind::Fragment, source);
    let source = legalized.source();

    assert!(source.contains("if (u + i)"));
    assert!(source.contains("while (u % i)"));
    assert!(!source.contains("u + i != 0"));
    assert!(!source.contains("u + i != 0u"));
    assert!(!source.contains("u + i != 0.0"));
    assert!(!source.contains("u % i != 0"));
    assert!(!source.contains("u % i != 0u"));
    assert!(!source.contains("u % i != 0.0"));
}
