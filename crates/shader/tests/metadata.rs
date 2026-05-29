use shader::{
    ComboName, DefaultUniformValue, PropertyValue, ShaderComboValue, ShaderMetadata,
    ShaderStageKind, ShaderTextureInfo, TextureComponentState, TextureFormatHint, TextureSlot,
    syntax::ShaderModule,
};

fn parse_metadata(source: &str, textures: &[ShaderTextureInfo]) -> ShaderMetadata {
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    module
        .extract_metadata(textures)
        .expect("metadata extracts")
}

fn texture(slot: u8, is_enabled: bool) -> ShaderTextureInfo {
    ShaderTextureInfo::new(
        TextureSlot::new(slot).expect("valid texture slot"),
        is_enabled,
        TextureFormatHint::Rgba8,
    )
}

fn texture_with_components(slot: u8, is_enabled: bool, components: [bool; 3]) -> ShaderTextureInfo {
    ShaderTextureInfo::with_components(
        TextureSlot::new(slot).expect("valid texture slot"),
        is_enabled,
        TextureFormatHint::Rgba8,
        components.map(TextureComponentState::new),
    )
}

#[test]
fn shader_module_extracts_metadata() {
    let source = r#"
// [COMBO] {"combo":"QUALITY","default":3}
void main(){}
"#;
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let metadata = module.extract_metadata(&[]).expect("metadata extracts");

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("QUALITY").expect("valid combo"),
            "3"
        )]
    );
}

#[test]
fn records_combo_defaults_before_main() {
    let metadata = parse_metadata(
        r#"
#if 0
// [COMBO] {"combo":"BLENDMODE","default":9}
#endif
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("BLENDMODE").expect("valid combo"),
            "9"
        )]
    );
}

#[test]
fn records_material_aliases_for_scalar_and_texture_uniforms() {
    let metadata = parse_metadata(
        r#"
uniform float g_Brightness; // {"material":"brightness","default":1.5}
uniform sampler2D g_Texture0; // {"material":"albedo","default":"util/white"}
void main(){}
"#,
        &[texture(0, true)],
    );

    assert_eq!(metadata.aliases().len(), 2);
    assert_eq!(metadata.aliases()[0].material(), "brightness");
    assert_eq!(metadata.aliases()[0].uniform(), "g_Brightness");
    assert_eq!(metadata.aliases()[1].material(), "albedo");
    assert_eq!(metadata.aliases()[1].uniform(), "g_Texture0");
}

#[test]
fn records_default_scalar_and_vector_uniforms() {
    let metadata = parse_metadata(
        r#"
layout(location = 0) uniform highp float g_Exposure; // {"default":2.0}
uniform vec3 g_Tint; // {"default":"0.25 0.5 0.75"}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.default_uniforms(),
        &[
            DefaultUniformValue::new("g_Exposure", PropertyValue::Number(2.0))
                .expect("valid default uniform"),
            DefaultUniformValue::new("g_Tint", PropertyValue::Vec3([0.25, 0.5, 0.75]))
                .expect("valid default uniform"),
        ]
    );
}

#[test]
fn records_vec2_metadata_defaults_as_typed_vectors() {
    let metadata = parse_metadata(
        r#"
uniform vec2 g_Scale; // {"default":"1 1","material":"scale"}
void main(){}
"#,
        &[],
    );

    assert_eq!(metadata.default_uniforms().len(), 1);
    assert_eq!(metadata.default_uniforms()[0].uniform(), "g_Scale");
    assert_eq!(
        metadata.default_uniforms()[0].value(),
        &PropertyValue::Vec2([1.0, 1.0])
    );
}

#[test]
fn parses_numeric_metadata_default_shapes_as_typed_values() {
    assert_eq!(
        PropertyValue::parse_metadata_default("2").expect("scalar parses"),
        PropertyValue::Number(2.0)
    );
    assert_eq!(
        PropertyValue::parse_metadata_default("1 2").expect("vec2 parses"),
        PropertyValue::Vec2([1.0, 2.0])
    );
    assert_eq!(
        PropertyValue::parse_metadata_default("1, 2, 3").expect("vec3 parses"),
        PropertyValue::Vec3([1.0, 2.0, 3.0])
    );
    assert_eq!(
        PropertyValue::parse_metadata_default("1 2 3 4").expect("vec4 parses"),
        PropertyValue::Vec4([1.0, 2.0, 3.0, 4.0])
    );
    assert_eq!(
        PropertyValue::parse_metadata_default("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16")
            .expect("matrix4 parses"),
        PropertyValue::Matrix4([
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ])
    );
    assert_eq!(
        PropertyValue::parse_metadata_default("1 2 3 4 5").expect("fallback parses"),
        PropertyValue::String("1 2 3 4 5".to_owned())
    );
}

#[test]
fn annotation_defaults_parse_into_typed_payloads_before_builder_use() {
    let metadata = parse_metadata(
        r#"
uniform float g_Exposure; // {"default":2.0}
uniform vec3 g_Tint; // {"default":"0.25, 0.5, 0.75"}
uniform bool g_Enabled; // {"default":true}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.default_uniforms(),
        &[
            DefaultUniformValue::new("g_Exposure", PropertyValue::Number(2.0))
                .expect("valid default uniform"),
            DefaultUniformValue::new("g_Tint", PropertyValue::Vec3([0.25, 0.5, 0.75]))
                .expect("valid default uniform"),
            DefaultUniformValue::new("g_Enabled", PropertyValue::Bool(true))
                .expect("valid default uniform"),
        ]
    );
}

#[test]
fn records_default_textures() {
    let metadata = parse_metadata(
        r#"
uniform sampler2D g_Texture6; // {"hidden":true,"default":"_rt_shadowAtlas"}
void main(){}
"#,
        &[texture(6, true)],
    );

    assert_eq!(metadata.default_textures().len(), 1);
    assert_eq!(metadata.default_textures()[0].slot().index(), 6);
    assert_eq!(metadata.default_textures()[0].path(), "_rt_shadowAtlas");
}

#[test]
fn records_texture_and_component_combos_from_enabled_texture_state() {
    let metadata = parse_metadata(
        r#"
uniform sampler2D g_Texture0; // {"combo":"HASTEX","components":[{"combo":"HAS_R"},{"combo":"HAS_G"},{"combo":"HAS_B"}]}
void main(){}
"#,
        &[texture_with_components(0, true, [true, false, true])],
    );

    assert_eq!(
        metadata.combos(),
        &[
            ShaderComboValue::new(ComboName::new("HASTEX").expect("valid combo"), "1"),
            ShaderComboValue::new(ComboName::new("HAS_R").expect("valid combo"), "1"),
            ShaderComboValue::new(ComboName::new("HAS_B").expect("valid combo"), "1"),
        ]
    );
}

#[test]
fn repeated_metadata_combos_replace_value_without_reordering() {
    let metadata = parse_metadata(
        r#"
// [COMBO] {"combo":"FIRST","default":0}
// [COMBO] {"combo":"SECOND","default":1}
// [COMBO] {"combo":"first","default":2}
void main(){}
"#,
        &[],
    );

    let combos = metadata.combos();

    assert_eq!(combos[0].name().as_str(), "first");
    assert_eq!(combos[0].value(), "2");
    assert_eq!(combos[1].name().as_str(), "SECOND");
    assert_eq!(combos[1].value(), "1");
}

#[test]
fn disabled_present_texture_slots_produce_one_texture_combo_and_no_component_combos() {
    let metadata = parse_metadata(
        r#"
uniform sampler2D g_Texture2; // {"combo":"MASK","components":[{"combo":"MASK_R"}]}
void main(){}
"#,
        &[texture_with_components(2, false, [true, true, true])],
    );

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("MASK").expect("valid combo"),
            "1"
        )]
    );
}

#[test]
fn missing_texture_slots_produce_zero_texture_combo() {
    let metadata = parse_metadata(
        r#"
uniform sampler2D g_Texture2; // {"combo":"MASK","components":[{"combo":"MASK_R"}]}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("MASK").expect("valid combo"),
            "0"
        )]
    );
}

#[test]
fn stops_scanning_metadata_at_void_main() {
    let metadata = parse_metadata(
        r#"
uniform float g_Before; // {"material":"before","default":1.0}
void main(){}
// [COMBO] {"combo":"AFTER","default":1}
uniform float g_After; // {"material":"after","default":2.0}
"#,
        &[],
    );

    assert_eq!(metadata.aliases().len(), 1);
    assert_eq!(metadata.aliases()[0].material(), "before");
    assert!(metadata.combos().is_empty());
    assert_eq!(metadata.default_uniforms().len(), 1);
    assert_eq!(metadata.default_uniforms()[0].uniform(), "g_Before");
}

#[test]
fn ignores_json_annotation_on_line_after_uniform_declaration() {
    let metadata = parse_metadata(
        r#"
uniform float g_A;
// {"material":"a","default":1.0}
// [COMBO] {"combo":"KEPT","default":2}
void main(){}
"#,
        &[],
    );

    assert!(metadata.aliases().is_empty());
    assert!(metadata.default_uniforms().is_empty());
    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("KEPT").expect("valid combo"),
            "2"
        )]
    );
}

#[test]
fn ignores_block_commented_metadata_but_preserves_code_after_same_line_block_comment() {
    let metadata = parse_metadata(
        r#"
/* uniform float g_X; // {"default":1} */
/*c*/ uniform float g_Y; // {"default":2}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.default_uniforms(),
        &[DefaultUniformValue::new("g_Y", PropertyValue::Number(2.0))
            .expect("valid default uniform")]
    );
}

#[test]
fn ignores_malformed_combo_annotation_without_rejecting_shader_metadata() {
    let metadata = parse_metadata(
        r#"
// [COMBO] {"combo":"GOOD","default":2}
// [COMBO] {"material":"Missing closing quote,"combo":"BROKEN","default":1}
uniform float g_Value; // {"default":3}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("GOOD").expect("valid combo"),
            "2"
        )]
    );
    assert_eq!(
        metadata.default_uniforms(),
        &[
            DefaultUniformValue::new("g_Value", PropertyValue::Number(3.0))
                .expect("valid default uniform")
        ]
    );
}

#[test]
fn ignores_combo_annotation_without_json_object() {
    let metadata = parse_metadata(
        r#"
// [COMBO] VALUE 0 1
// [COMBO] {"combo":"GOOD","default":2}
void main(){}
"#,
        &[],
    );

    assert_eq!(
        metadata.combos(),
        &[ShaderComboValue::new(
            ComboName::new("GOOD").expect("valid combo"),
            "2"
        )]
    );
}
