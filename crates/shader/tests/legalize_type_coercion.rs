use shader::{
    ShaderCompiler, ShaderStageKind,
    compile::NagaCompiler,
    legalize::{LegalizedStageSource, Legalizer},
    syntax::ShaderModule,
};

fn legalize(stage: ShaderStageKind, source: &str) -> LegalizedStageSource {
    let module = ShaderModule::parse(stage, source).expect("module parses");
    Legalizer.legalize(&module).expect("shader legalizes")
}

#[test]
fn type_coercion_policy_widens_vec2_constructor_in_vec3_binary_expression() {
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
fn type_coercion_policy_broadcasts_shadowed_scalar_identifier() {
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
}

#[test]
fn type_coercion_policy_broadcasts_scalar_literal_before_swizzled_vector_max() {
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
fn type_coercion_policy_broadcasts_scalar_literal_before_swizzled_texture_sample() {
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
fn type_coercion_policy_uses_nearest_vector_width_for_vec3_vec2_widening() {
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
fn type_coercion_policy_stops_vec3_vec2_widening_at_aggregate_local_blocker() {
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
fn type_coercion_policy_stops_builtin_scalar_broadcast_at_aggregate_local_blocker() {
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
fn type_coercion_policy_stops_vec3_vec2_widening_at_aggregate_parameter_blocker() {
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
fn type_coercion_policy_tracks_common_aggregate_blocker_spellings() {
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
fn control_flow_policy_casts_integer_for_loop_bounds() {
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
fn control_flow_policy_casts_integer_for_loop_inclusive_bounds() {
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
fn control_flow_policy_does_not_cast_float_for_loop_bounds() {
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
fn control_flow_policy_uses_nearest_binding_for_bool_coercion() {
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
fn control_flow_policy_tracks_bool_comma_declarators() {
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
fn control_flow_policy_tracks_comment_separated_bool_declarations() {
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
fn control_flow_policy_ignores_function_prototype_parameters() {
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
fn control_flow_policy_tracks_function_body_bool_parameters() {
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
fn control_flow_policy_uses_type_appropriate_numeric_condition_zero_literals() {
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
fn control_flow_policy_uses_unsigned_zero_for_unsigned_arithmetic_conditions() {
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
fn control_flow_policy_leaves_mixed_signedness_arithmetic_conditions_unrewritten() {
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
