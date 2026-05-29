use shader::{
    ComboName, InMemoryShaderSourceProvider, IncludePath, ShaderComboValue, ShaderError,
    ShaderName, ShaderProgramRequest, ShaderStageKind, ShaderStageSource,
    preprocess::PreprocessContext, syntax::ShaderModule,
};

fn include(path: &str) -> IncludePath {
    IncludePath::new(path).expect("valid include path")
}

fn combo(name: &str, value: &str) -> ShaderComboValue {
    ShaderComboValue::new(ComboName::new(name).expect("valid combo name"), value)
}

fn request_with_fragment(source: &str, combos: &[ShaderComboValue]) -> ShaderProgramRequest {
    let mut builder = ShaderProgramRequest::builder(
        ShaderName::new("effects/preprocess").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}\n",
    ))
    .stage(ShaderStageSource::new(ShaderStageKind::Fragment, source));

    for shader_combo in combos {
        builder = builder.combo(shader_combo.clone());
    }

    builder.build().expect("valid shader request")
}

fn parse_error_message(error: &ShaderError) -> String {
    match error {
        ShaderError::Parse { diagnostics } => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message().to_owned())
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

#[test]
fn expands_recursive_includes_through_source_provider() {
    let provider = InMemoryShaderSourceProvider::new()
        .with_source(
            include("common/root.glsl"),
            "#include \"common/math.glsl\"\nfloat root = math_value;\n",
        )
        .with_source(include("common/math.glsl"), "float math_value = 4.0;\n");
    let request = request_with_fragment(
        "#include \"common/root.glsl\"\nvoid main() { float value = root; }\n",
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let fragment = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists");

    assert!(fragment.source().contains("float math_value = 4.0;"));
    assert!(fragment.source().contains("float root = math_value;"));
    assert!(fragment.source().contains("void main()"));
    assert!(!fragment.source().contains("#include"));
}

#[test]
fn preprocess_context_method_preprocesses_program() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment("void main() {}\n", &[]);

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses through context method");

    assert!(program.stage(ShaderStageKind::Vertex).is_some());
    assert!(program.stage(ShaderStageKind::Fragment).is_some());
}

#[test]
fn default_wallpaper_engine_macros_select_glsl_paths() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if GLSL
float glsl_path = 1.0;
#else
float glsl_path = 0.0;
#endif
#ifdef HLSL
float hlsl_defined = 1.0;
#else
float hlsl_defined = 0.0;
#endif
#if HLSL
float hlsl_truthy = 1.0;
#else
float hlsl_truthy = 0.0;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float glsl_path = 1.0;"));
    assert!(source.contains("float hlsl_defined = 1.0;"));
    assert!(source.contains("float hlsl_truthy = 0.0;"));
    assert!(!source.contains("float glsl_path = 0.0;"));
    assert!(!source.contains("float hlsl_defined = 0.0;"));
    assert!(!source.contains("float hlsl_truthy = 1.0;"));
}

#[test]
fn preserves_function_like_defines_and_records_macro_name() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define BlendLinearDodgef(base, blend) (base + blend)
#if BlendLinearDodgef
float blend_macro_defined = 1.0;
#else
float blend_macro_defined = 0.0;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("#define BlendLinearDodgef(base, blend) (base + blend)"));
    assert!(source.contains("float blend_macro_defined = 1.0;"));
    assert!(!source.contains("float blend_macro_defined = 0.0;"));
}

#[test]
fn reports_include_cycles_with_chain_context() {
    let provider = InMemoryShaderSourceProvider::new()
        .with_source(include("a.glsl"), "\n#include \"b.glsl\"\n")
        .with_source(include("b.glsl"), "\n\n#include \"a.glsl\"\n");
    let request = request_with_fragment("#include \"a.glsl\"\nvoid main() {}\n", &[]);

    let error = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect_err("include cycle should fail");
    let message = parse_error_message(&error);

    assert!(message.contains("include cycle"));
    assert!(message.contains("a.glsl"));
    assert!(message.contains("b.glsl"));
    assert!(message.contains("include b.glsl line 3"));
}

#[test]
fn unquoted_include_reports_quoted_path_diagnostic() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment("#include common.glsl\nvoid main() {}\n", &[]);

    let error = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect_err("unquoted include should fail");
    let message = parse_error_message(&error);

    assert!(message.contains("#include expects a quoted include path"));
}

#[test]
fn doubled_hash_directives_are_preserved_as_unknown_directives() {
    let provider = InMemoryShaderSourceProvider::new()
        .with_source(include("common.glsl"), "float leak = 1.0;\n");
    let request = request_with_fragment(
        concat!(
            "##include \"common.glsl\"\n",
            "##define DOUBLED 1\n",
            "##if DOUBLED\n",
            "float kept = 1.0;\n",
            "##endif\n",
            "void main() {}\n",
        ),
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("##include \"common.glsl\""));
    assert!(source.contains("##define DOUBLED 1"));
    assert!(source.contains("##if DOUBLED"));
    assert!(source.contains("##endif"));
    assert!(source.contains("float kept = 1.0;"));
    assert!(!source.contains("float leak = 1.0;"));
}

#[test]
fn evaluates_defines_and_conditional_directives() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define ENABLE_FOG 1
#define QUALITY 2
#ifdef ENABLE_FOG
float fog = 1.0;
#else
this is not glsl;
#endif
#ifndef ENABLE_SHADOW
float shadow = 0.0;
#else
this is not glsl either;
#endif
#if ENABLE_FOG
float enabled = 1.0;
#endif
#if QUALITY == 2
float high = 1.0;
#else
this inactive line is invalid glsl;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float fog = 1.0;"));
    assert!(source.contains("float shadow = 0.0;"));
    assert!(source.contains("float enabled = 1.0;"));
    assert!(source.contains("float high = 1.0;"));
    assert!(!source.contains("not glsl"));
}

#[test]
fn preserves_active_defines_and_omits_inactive_defines() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define SCALE 2.0
#if 0
#define HIDDEN 1
#endif
float x = SCALE;
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("#define SCALE 2.0"));
    assert!(source.contains("float x = SCALE;"));
    assert!(!source.contains("#define HIDDEN"));
}

#[test]
fn typed_directives_feed_include_and_define_preprocessing() {
    let source = concat!(
        "#include \"common/math.glsl\" // keep URL-like paths intact\n",
        "#define VALUE 7 // define comment\n",
        "float x = VALUE;\n",
        "void main() {}\n",
    );
    let provider = InMemoryShaderSourceProvider::new()
        .with_source(include("common/math.glsl"), "float from_include = 1.0;\n");
    let request = request_with_fragment(source, &[]);
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let directive_kinds: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            shader::syntax::SyntaxItem::Directive(directive) => Some(directive.kind()),
            _ => None,
        })
        .collect();

    assert!(directive_kinds[0].is_include());
    assert_eq!(
        directive_kinds[0].body().include_path_text(),
        Some("common/math.glsl")
    );
    assert!(directive_kinds[1].is_define());
    assert_eq!(directive_kinds[1].name().as_str(), "define");
    assert_eq!(directive_kinds[1].body().as_str(), "VALUE 7");

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let fragment = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists");

    assert!(fragment.source().contains("float from_include = 1.0;"));
    assert!(fragment.source().contains("#define VALUE 7"));
    assert!(fragment.source().contains("float x = VALUE;"));
}

#[test]
fn if_equality_resolves_symbolic_rhs_macro_values() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define BOTTOM 0
#if SHAPE == BOTTOM
float bottom = 1.0;
#else
float bottom = 0.0;
#endif
void main() {}
"#,
        &[combo("SHAPE", "0")],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float bottom = 1.0;"));
    assert!(!source.contains("float bottom = 0.0;"));
}

#[test]
fn elif_selects_first_active_branch() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if 0
float selected = 0.0;
#elif 1
float selected = 1.0;
#else
float selected = 2.0;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float selected = 1.0;"));
    assert!(!source.contains("float selected = 0.0;"));
    assert!(!source.contains("float selected = 2.0;"));
}

#[test]
fn elif_skips_later_true_branch_after_prior_branch_selected() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if 1
float selected = 1.0;
#elif 1
float selected = 2.0;
#else
float selected = 3.0;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float selected = 1.0;"));
    assert!(!source.contains("float selected = 2.0;"));
    assert!(!source.contains("float selected = 3.0;"));
}

#[test]
fn require_directive_is_dropped() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
float before_require = 1.0;
#require something
float after_require = 1.0;
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float before_require = 1.0;"));
    assert!(source.contains("float after_require = 1.0;"));
    assert!(!source.contains("#require"));
}

#[test]
fn combo_names_are_available_as_uppercase_macros() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if BLENDMODE == 2
float selected = 2.0;
#else
float selected = 0.0;
#endif
void main() {}
"#,
        &[combo("blendmode", "2")],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float selected = 2.0;"));
    assert!(!source.contains("float selected = 0.0;"));
}

#[test]
fn directive_arguments_tolerate_trailing_line_comments() {
    let provider = InMemoryShaderSourceProvider::new().with_source(
        include("http//shader/common.glsl"),
        "float from_include = 1.0;\n",
    );
    let request = request_with_fragment(
        r#"
#define QUALITY 2
#include "http//shader/common.glsl" // trailing include comment
#if QUALITY == 2 // trailing if comment
float quality = from_include;
#else // trailing else comment
float quality = 0.0;
#endif // trailing endif comment
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float from_include = 1.0;"));
    assert!(source.contains("float quality = from_include;"));
    assert!(!source.contains("float quality = 0.0;"));
}

#[test]
fn tolerates_stray_trailing_endif_directive() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
float value = 1.0;
void main() {}
#endif
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float value = 1.0;"));
    assert!(source.contains("void main()"));
}

#[test]
fn tolerates_unmatched_endif_before_later_code_like_legacy_preprocessor() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if ENABLED
float selected = 1.0;
#endif
#endif
float after_stray_endif = 1.0;
void main() {}
"#,
        &[combo("ENABLED", "1")],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float selected = 1.0;"));
    assert!(source.contains("float after_stray_endif = 1.0;"));
    assert!(source.contains("void main()"));
}

#[test]
fn request_combos_are_visible_to_condition_evaluation() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if BLENDMODE == 2
float selected = 2.0;
#else
float selected = 0.0;
#endif
#if HAS_MASK
float masked = 1.0;
#endif
void main() {}
"#,
        &[combo("BLENDMODE", "2"), combo("HAS_MASK", "1")],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float selected = 2.0;"));
    assert!(source.contains("float masked = 1.0;"));
    assert!(!source.contains("float selected = 0.0;"));
}

#[test]
fn request_combos_are_emitted_as_macros_for_live_shader_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        concat!(
            "float ApplyBlending(int mode, float base, float layer) {\n",
            "    return mode == 9 ? layer : base;\n",
            "}\n",
            "void main() {\n",
            "    float blended = ApplyBlending(BLENDMODE, 0.25, 0.75);\n",
            "}\n",
        ),
        &[combo("BLENDMODE", "9")],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("#define BLENDMODE 9"));
    assert!(source.contains("ApplyBlending(BLENDMODE, 0.25, 0.75)"));
}

#[test]
fn evaluates_wallpaper_engine_boolean_if_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if LIGHTING || REFLECTION
float lighting_or_reflection = 1.0;
#else
float lighting_or_reflection = 0.0;
#endif
#if FOG_DIST || FOG_HEIGHT || LIGHTING
float fog_or_lighting = 1.0;
#else
float fog_or_lighting = 0.0;
#endif
#if (LIGHTING || REFLECTION) && EMISSIVE_MAP
float emissive = 1.0;
#else
float emissive = 0.0;
#endif
void main() {}
"#,
        &[
            combo("LIGHTING", "0"),
            combo("REFLECTION", "1"),
            combo("FOG_DIST", "0"),
            combo("FOG_HEIGHT", "1"),
            combo("EMISSIVE_MAP", "1"),
        ],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float lighting_or_reflection = 1.0;"));
    assert!(source.contains("float fog_or_lighting = 1.0;"));
    assert!(source.contains("float emissive = 1.0;"));
    assert!(!source.contains("float lighting_or_reflection = 0.0;"));
    assert!(!source.contains("float fog_or_lighting = 0.0;"));
    assert!(!source.contains("float emissive = 0.0;"));
}

#[test]
fn evaluates_wallpaper_engine_numeric_if_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define FORMAT_ETC1_RGB8 10
#define FORMAT_DXT1 14
#define FORMAT_BC7 20
#if NORMALMAP == 0
float no_normal_map = 1.0;
#else
float no_normal_map = 0.0;
#endif
#if TEX1FORMAT >= FORMAT_ETC1_RGB8 && TEX1FORMAT <= FORMAT_DXT1 || TEX1FORMAT == FORMAT_BC7
float supported_texture = 1.0;
#else
float supported_texture = 0.0;
#endif
#if TRAILSUBDIVISION != 0
float trail_subdivision = 1.0;
#else
float trail_subdivision = 0.0;
#endif
void main() {}
"#,
        &[
            combo("NORMALMAP", "0"),
            combo("TEX1FORMAT", "12"),
            combo("TRAILSUBDIVISION", "2"),
        ],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float no_normal_map = 1.0;"));
    assert!(source.contains("float supported_texture = 1.0;"));
    assert!(source.contains("float trail_subdivision = 1.0;"));
    assert!(!source.contains("float no_normal_map = 0.0;"));
    assert!(!source.contains("float supported_texture = 0.0;"));
    assert!(!source.contains("float trail_subdivision = 0.0;"));
}

#[test]
fn undefined_identifiers_evaluate_as_zero_in_if_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if MISSING
float missing_truthy = 1.0;
#else
float missing_truthy = 0.0;
#endif
#if !MISSING
float missing_negated = 1.0;
#else
float missing_negated = 0.0;
#endif
#if MISSING_VALUE == 0
float missing_numeric = 1.0;
#else
float missing_numeric = 0.0;
#endif
void main() {}
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(source.contains("float missing_truthy = 0.0;"));
    assert!(source.contains("float missing_negated = 1.0;"));
    assert!(source.contains("float missing_numeric = 1.0;"));
    assert!(!source.contains("float missing_truthy = 1.0;"));
    assert!(!source.contains("float missing_negated = 0.0;"));
    assert!(!source.contains("float missing_numeric = 0.0;"));
}

#[test]
fn removes_inactive_code_before_naga_parsing() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#define USE_BAD_CODE 0
#if USE_BAD_CODE
this branch should never reach naga;
#else
void main() {}
#endif
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(!source.contains("this branch should never reach naga"));
    let _module =
        ShaderModule::parse(ShaderStageKind::Fragment, source).expect("inactive code was removed");
}

#[test]
fn malformed_conditionals_return_parse_diagnostics() {
    let provider = InMemoryShaderSourceProvider::new();
    let unmatched_else = request_with_fragment("#else\nvoid main() {}\n", &[]);
    let duplicate_else = request_with_fragment(
        "#if 1\nfloat a = 1.0;\n#else\nfloat a = 0.0;\n#else\n#endif\n",
        &[],
    );
    let unterminated_if = request_with_fragment("\n\n#if FEATURE\nvoid main() {}\n", &[]);

    let else_error = PreprocessContext::new(&unmatched_else, &provider)
        .preprocess()
        .expect_err("unmatched else should fail");
    let duplicate_else_error = PreprocessContext::new(&duplicate_else, &provider)
        .preprocess()
        .expect_err("duplicate else should fail");
    let if_error = PreprocessContext::new(&unterminated_if, &provider)
        .preprocess()
        .expect_err("unterminated if should fail");

    assert!(matches!(else_error, ShaderError::Parse { .. }));
    assert!(parse_error_message(&else_error).contains("unmatched #else"));
    assert!(matches!(duplicate_else_error, ShaderError::Parse { .. }));
    assert!(parse_error_message(&duplicate_else_error).contains("duplicate #else"));
    assert!(matches!(if_error, ShaderError::Parse { .. }));
    let if_message = parse_error_message(&if_error);
    assert!(if_message.contains("unterminated conditional"));
    assert!(if_message.contains("line 3"));
}

#[test]
fn inactive_includes_are_not_read_and_nested_inactive_conditionals_stay_inactive() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if 0
#include "missing.glsl"
#if 1
this nested inactive branch is invalid glsl;
#endif
#else
void main() {}
#endif
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(!source.contains("nested inactive"));
    assert!(source.contains("void main()"));
}

#[test]
fn inactive_parent_conditionals_do_not_validate_nested_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if 0
#if defined(FOO) || (A && B)
this nested inactive branch is invalid glsl;
#endif
#else
void main() {}
#endif
"#,
        &[],
    );

    let program = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect("preprocesses");
    let source = program
        .stage(ShaderStageKind::Fragment)
        .expect("fragment stage exists")
        .source();

    assert!(!source.contains("nested inactive"));
    assert!(source.contains("void main()"));
}

#[test]
fn active_parent_conditionals_still_reject_unsupported_expressions() {
    let provider = InMemoryShaderSourceProvider::new();
    let request = request_with_fragment(
        r#"
#if 1
#if defined(FOO) || (A && B)
void main() {}
#endif
#endif
"#,
        &[],
    );

    let error = PreprocessContext::new(&request, &provider)
        .preprocess()
        .expect_err("unsupported active expression should fail");
    let message = parse_error_message(&error);

    assert!(message.contains("#if expression is unsupported"));
}
