use shader::{
    BindingIndex, BindingSet, ComboName, CompiledShaderStage, CompiledStageArtifact,
    InMemoryShaderSourceProvider, IncludePath, LocationIndex, ProjectPropertyBinding, PropertyName,
    PropertyValue, ShaderCachePolicy, ShaderComboValue, ShaderCompiler, ShaderDiagnostic,
    ShaderError, ShaderName, ShaderProgramRequest, ShaderReflection, ShaderReflector, ShaderResult,
    ShaderSourceProvider, ShaderStageKind, ShaderStageSource, ShaderTarget, ShaderTextureInfo,
    TextureFormatHint, TextureSlot, legalize::LegalizedStageSource,
};

#[test]
fn builds_typed_shader_request_and_reads_include_source() {
    let shader_name = ShaderName::new("effects/genericimage").expect("valid shader name");
    let vertex_source = ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "#include \"common/shared.glsl\"\nvoid main() {}",
    );
    let fragment_source = ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "uniform sampler2D g_Texture0;\nvoid main() {}",
    );
    let combo = ShaderComboValue::new(ComboName::new("HAS_ALPHA").expect("valid combo"), "1");
    let texture = ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    );
    let property = ProjectPropertyBinding::new(
        PropertyName::new("opacity").expect("valid property name"),
        PropertyValue::Number(0.75),
    );

    let request = ShaderProgramRequest::builder(shader_name.clone())
        .stage(vertex_source)
        .stage(fragment_source)
        .combo(combo)
        .texture(texture)
        .property(property)
        .target(ShaderTarget::VulkanSpirv)
        .cache_policy(ShaderCachePolicy::Enabled {
            scene_id: "3611439897".to_owned(),
        })
        .build()
        .expect("request should be valid");

    assert_eq!(request.shader_name(), &shader_name);
    assert_eq!(request.stages().len(), 2);
    assert_eq!(request.combos().len(), 1);
    assert_eq!(request.textures().len(), 1);
    assert_eq!(request.properties().len(), 1);
    assert_eq!(request.target(), ShaderTarget::VulkanSpirv);
    assert_eq!(
        request.cache_policy(),
        &ShaderCachePolicy::Enabled {
            scene_id: "3611439897".to_owned()
        }
    );

    let include_path = IncludePath::new("common/shared.glsl").expect("valid include path");
    let provider = InMemoryShaderSourceProvider::new().with_source(
        include_path.clone(),
        "vec2 shared_uv(vec2 uv) { return uv; }",
    );

    let include_source = ShaderSourceProvider::read_to_string(&provider, &include_path)
        .expect("include should be present");

    assert_eq!(include_source, "vec2 shared_uv(vec2 uv) { return uv; }");
}

#[test]
fn builder_rejects_duplicate_combos_unless_replaced() {
    let request_error = ShaderProgramRequest::builder(
        ShaderName::new("effects/genericimage").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}",
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "void main() {}",
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("BLENDMODE").expect("valid combo"),
        "0",
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("BLENDMODE").expect("valid combo"),
        "1",
    ))
    .build()
    .expect_err("duplicate combo should fail");

    assert_eq!(
        request_error.to_string(),
        "invalid shader request: duplicate combo name blendmode"
    );

    let request = ShaderProgramRequest::builder(
        ShaderName::new("effects/genericimage").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}",
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "void main() {}",
    ))
    .combo(ShaderComboValue::new(
        ComboName::new("BLENDMODE").expect("valid combo"),
        "0",
    ))
    .replace_combo(ShaderComboValue::new(
        ComboName::new("BLENDMODE").expect("valid combo"),
        "1",
    ))
    .build()
    .expect("replacement should be valid");

    assert_eq!(request.combos().len(), 1);
    assert_eq!(request.combos()[0].value(), "1");
}

#[test]
fn reflector_trait_returns_core_reflection_model() {
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

    let reflection = EmptyReflector
        .reflect_stage(ShaderStageKind::Vertex, &())
        .expect("reflection should be produced");

    assert!(reflection.descriptor_bindings().is_empty());
    assert!(reflection.uniform_blocks().is_empty());
    assert!(reflection.vertex_inputs().is_empty());
    assert!(reflection.active_texture_slots().is_empty());
}

#[test]
fn numeric_newtypes_reject_values_outside_renderer_limits() {
    assert_eq!(
        LocationIndex::new(LocationIndex::MAX + 1)
            .expect_err("location above renderer limit should fail")
            .to_string(),
        "invalid shader request: location index is out of range"
    );
    assert_eq!(
        BindingSet::new(BindingSet::MAX + 1)
            .expect_err("binding set above renderer limit should fail")
            .to_string(),
        "invalid shader request: binding set is out of range"
    );
    assert_eq!(
        BindingIndex::new(BindingIndex::MAX + 1)
            .expect_err("binding index above renderer limit should fail")
            .to_string(),
        "invalid shader request: binding index is out of range"
    );

    assert_eq!(
        LocationIndex::new(LocationIndex::MAX)
            .expect("max location should be valid")
            .index(),
        LocationIndex::MAX
    );
    assert_eq!(
        BindingSet::new(BindingSet::MAX)
            .expect("max binding set should be valid")
            .set(),
        BindingSet::MAX
    );
    assert_eq!(
        BindingIndex::new(BindingIndex::MAX)
            .expect("max binding index should be valid")
            .binding(),
        BindingIndex::MAX
    );
}

#[test]
fn include_path_rejects_absolute_drive_unc_and_parent_paths() {
    for path in [
        "/absolute.glsl",
        "C:/wallpaper/shader.glsl",
        "C:\\wallpaper\\shader.glsl",
        "C:wallpaper/shader.glsl",
        "//server/share/shader.glsl",
        "\\\\server\\share\\shader.glsl",
        "common/../secret.glsl",
    ] {
        assert!(
            IncludePath::new(path).is_err(),
            "{path} should not be a valid include path"
        );
    }
}

#[test]
fn request_builder_rejects_missing_duplicate_and_none_property_inputs() {
    let missing_fragment = ShaderProgramRequest::builder(
        ShaderName::new("effects/genericimage").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}",
    ))
    .build()
    .expect_err("missing fragment should fail");

    assert_eq!(
        missing_fragment.to_string(),
        "invalid shader request: shader request missing fragment stage"
    );

    let duplicate_texture = ShaderProgramRequest::builder(
        ShaderName::new("effects/genericimage").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}",
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "void main() {}",
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        true,
        TextureFormatHint::Rgba8,
    ))
    .texture(ShaderTextureInfo::new(
        TextureSlot::new(0).expect("valid texture slot"),
        false,
        TextureFormatHint::Unknown,
    ))
    .build()
    .expect_err("duplicate texture slot should fail");

    assert_eq!(
        duplicate_texture.to_string(),
        "invalid shader request: duplicate texture slot 0"
    );

    let none_property = ShaderProgramRequest::builder(
        ShaderName::new("effects/genericimage").expect("valid shader name"),
    )
    .stage(ShaderStageSource::new(
        ShaderStageKind::Vertex,
        "void main() {}",
    ))
    .stage(ShaderStageSource::new(
        ShaderStageKind::Fragment,
        "void main() {}",
    ))
    .property(ProjectPropertyBinding::new(
        PropertyName::new("opacity").expect("valid property name"),
        PropertyValue::None,
    ))
    .build()
    .expect_err("none property should fail");

    assert_eq!(
        none_property.to_string(),
        "invalid shader request: property opacity has no value"
    );
}

#[test]
fn source_read_error_variants_include_path_context() {
    let path = IncludePath::new("common/shared.glsl").expect("valid include path");
    let read_error = ShaderError::source_read(path.clone(), "permission denied");
    let utf8_error = ShaderError::invalid_source_utf8(path.clone());

    assert_eq!(
        read_error.to_string(),
        "shader source read failed for common/shared.glsl: permission denied"
    );
    assert_eq!(
        utf8_error.to_string(),
        "shader source utf-8 invalid for common/shared.glsl"
    );
}

#[test]
fn compiler_trait_returns_artifact_with_backend_module() {
    struct UnitCompiler;

    impl ShaderCompiler for UnitCompiler {
        type Module = &'static str;

        fn compile_stage(
            &self,
            stage: ShaderStageKind,
            _source: &LegalizedStageSource,
        ) -> ShaderResult<CompiledStageArtifact<Self::Module>> {
            let compiled_stage =
                CompiledShaderStage::new(stage, Box::from([0x0723_0203]), None, Box::from([]));
            Ok(CompiledStageArtifact::new(
                compiled_stage,
                "backend-module",
                Box::from([ShaderDiagnostic::new("compiled")]),
            ))
        }
    }

    let source = LegalizedStageSource::new(
        ShaderStageKind::Fragment,
        "void main() {}".to_owned(),
        Box::from([]),
    );
    let artifact = UnitCompiler
        .compile_stage(ShaderStageKind::Fragment, &source)
        .expect("compiler should return an artifact");

    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
    assert_eq!(artifact.stage().spirv(), &[0x0723_0203]);
    assert_eq!(artifact.module(), &"backend-module");
    assert_eq!(artifact.diagnostics()[0].message(), "compiled");
}

#[cfg(feature = "serde")]
mod serde_invariants {
    use serde::Deserialize;
    use serde_test::{Token, assert_de_tokens, assert_de_tokens_error};
    use shader::{
        BindingIndex, BindingSet, ComboName, IncludePath, LocationIndex, ProjectPropertyBinding,
        PropertyName, PropertyValue, ShaderCachePolicy, ShaderComboValue, ShaderDescriptorBinding,
        ShaderDescriptorKind, ShaderName, ShaderProgramRequest, ShaderReflection, ShaderStageKind,
        ShaderStageMask, ShaderStageSource, ShaderTarget, ShaderUniformBlock, ShaderUniformMember,
        ShaderVertexInput, SourceSpan, TextureFormatHint, TextureSlot, VertexFormat,
    };

    #[derive(Debug, Deserialize)]
    struct NameFixture {
        #[serde(rename = "name")]
        _name: ShaderName,
    }

    #[derive(Debug, Deserialize)]
    struct IncludeFixture {
        #[serde(rename = "path")]
        _path: IncludePath,
    }

    #[test]
    fn serde_rejects_invalid_newtypes_through_public_constructors() {
        assert_de_tokens_error::<NameFixture>(
            &[
                Token::Struct {
                    name: "NameFixture",
                    len: 1,
                },
                Token::Str("name"),
                Token::Str(""),
                Token::StructEnd,
            ],
            "invalid shader request: shader name is empty",
        );

        assert_de_tokens_error::<IncludeFixture>(
            &[
                Token::Struct {
                    name: "IncludeFixture",
                    len: 1,
                },
                Token::Str("path"),
                Token::Str("C:/absolute.glsl"),
                Token::StructEnd,
            ],
            "invalid shader request: include path has drive prefix",
        );
    }

    #[test]
    fn serde_rejects_invalid_shader_program_requests() {
        assert_de_tokens_error::<ShaderProgramRequest>(
            &[
                Token::Struct {
                    name: "ShaderProgramRequest",
                    len: 7,
                },
                Token::Str("shader_name"),
                Token::Str("effects/genericimage"),
                Token::Str("stages"),
                Token::Seq { len: Some(1) },
                Token::Struct {
                    name: "ShaderStageSource",
                    len: 2,
                },
                Token::Str("kind"),
                Token::UnitVariant {
                    name: "ShaderStageKind",
                    variant: "Vertex",
                },
                Token::Str("source"),
                Token::Str("void main() {}"),
                Token::StructEnd,
                Token::SeqEnd,
                Token::Str("combos"),
                Token::Seq { len: Some(0) },
                Token::SeqEnd,
                Token::Str("textures"),
                Token::Seq { len: Some(0) },
                Token::SeqEnd,
                Token::Str("properties"),
                Token::Seq { len: Some(0) },
                Token::SeqEnd,
                Token::Str("target"),
                Token::UnitVariant {
                    name: "ShaderTarget",
                    variant: "VulkanSpirv",
                },
                Token::Str("cache_policy"),
                Token::UnitVariant {
                    name: "ShaderCachePolicy",
                    variant: "Disabled",
                },
                Token::StructEnd,
            ],
            "invalid shader request: shader request missing fragment stage",
        );
    }

    #[test]
    fn serde_rejects_invalid_source_spans() {
        assert_de_tokens_error::<SourceSpan>(
            &[
                Token::Struct {
                    name: "SourceSpan",
                    len: 2,
                },
                Token::Str("start"),
                Token::U64(10),
                Token::Str("end"),
                Token::U64(4),
                Token::StructEnd,
            ],
            "invalid shader request: source span end is before start",
        );
    }

    #[test]
    fn serde_accepts_valid_shader_program_requests() {
        let request = ShaderProgramRequest::builder(
            ShaderName::new("effects/genericimage").expect("valid shader name"),
        )
        .stage(ShaderStageSource::new(
            ShaderStageKind::Vertex,
            "void main() {}",
        ))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Fragment,
            "void main() {}",
        ))
        .combo(ShaderComboValue::new(
            ComboName::new("HAS_ALPHA").expect("valid combo"),
            "1",
        ))
        .texture(shader::ShaderTextureInfo::new(
            TextureSlot::new(0).expect("valid texture slot"),
            true,
            TextureFormatHint::Rgba8,
        ))
        .property(ProjectPropertyBinding::new(
            PropertyName::new("opacity").expect("valid property name"),
            PropertyValue::Number(0.75),
        ))
        .target(ShaderTarget::VulkanSpirv)
        .cache_policy(ShaderCachePolicy::Disabled)
        .build()
        .expect("request should be valid");

        assert_eq!(request.stages().len(), 2);
        assert_eq!(request.combos().len(), 1);
        assert_eq!(request.textures().len(), 1);
        assert_eq!(request.properties().len(), 1);

        assert_de_tokens(
            &request,
            &[
                Token::Struct {
                    name: "ShaderProgramRequest",
                    len: 7,
                },
                Token::Str("shader_name"),
                Token::Str("effects/genericimage"),
                Token::Str("stages"),
                Token::Seq { len: Some(2) },
                Token::Struct {
                    name: "ShaderStageSource",
                    len: 2,
                },
                Token::Str("kind"),
                Token::UnitVariant {
                    name: "ShaderStageKind",
                    variant: "Vertex",
                },
                Token::Str("source"),
                Token::Str("void main() {}"),
                Token::StructEnd,
                Token::Struct {
                    name: "ShaderStageSource",
                    len: 2,
                },
                Token::Str("kind"),
                Token::UnitVariant {
                    name: "ShaderStageKind",
                    variant: "Fragment",
                },
                Token::Str("source"),
                Token::Str("void main() {}"),
                Token::StructEnd,
                Token::SeqEnd,
                Token::Str("combos"),
                Token::Seq { len: Some(1) },
                Token::Struct {
                    name: "ShaderComboValue",
                    len: 2,
                },
                Token::Str("name"),
                Token::Str("HAS_ALPHA"),
                Token::Str("value"),
                Token::Str("1"),
                Token::StructEnd,
                Token::SeqEnd,
                Token::Str("textures"),
                Token::Seq { len: Some(1) },
                Token::Struct {
                    name: "ShaderTextureInfo",
                    len: 4,
                },
                Token::Str("slot"),
                Token::U8(0),
                Token::Str("is_present"),
                Token::Bool(true),
                Token::Str("is_enabled"),
                Token::Bool(true),
                Token::Str("format"),
                Token::UnitVariant {
                    name: "TextureFormatHint",
                    variant: "Rgba8",
                },
                Token::StructEnd,
                Token::SeqEnd,
                Token::Str("properties"),
                Token::Seq { len: Some(1) },
                Token::Struct {
                    name: "ProjectPropertyBinding",
                    len: 2,
                },
                Token::Str("name"),
                Token::Str("opacity"),
                Token::Str("value"),
                Token::NewtypeVariant {
                    name: "PropertyValue",
                    variant: "Number",
                },
                Token::F32(0.75),
                Token::StructEnd,
                Token::SeqEnd,
                Token::Str("target"),
                Token::UnitVariant {
                    name: "ShaderTarget",
                    variant: "VulkanSpirv",
                },
                Token::Str("cache_policy"),
                Token::UnitVariant {
                    name: "ShaderCachePolicy",
                    variant: "Disabled",
                },
                Token::StructEnd,
            ],
        );
    }

    #[test]
    fn serde_serializes_reflection_contract_names() {
        let uniform_descriptor = ShaderDescriptorBinding::new(
            "GlobalUniforms",
            BindingSet::new(0).expect("valid set"),
            BindingIndex::new(1).expect("valid binding"),
            ShaderDescriptorKind::UniformBuffer,
            ShaderStageMask::new(true, true),
            1,
        )
        .expect("descriptor should be valid");
        let image_descriptor = ShaderDescriptorBinding::new(
            "g_Texture0",
            BindingSet::new(0).expect("valid set"),
            BindingIndex::new(2).expect("valid binding"),
            ShaderDescriptorKind::SampledImage,
            ShaderStageMask::new(false, true),
            1,
        )
        .expect("sampled image descriptor should be valid");
        let sampler_descriptor = ShaderDescriptorBinding::new(
            "_we_Sampler_g_Texture0",
            BindingSet::new(0).expect("valid set"),
            BindingIndex::new(3).expect("valid binding"),
            ShaderDescriptorKind::Sampler,
            ShaderStageMask::new(false, true),
            1,
        )
        .expect("sampler descriptor should be valid");
        let member =
            ShaderUniformMember::new("g_Mvp", 0, 64, 16, 0, 0).expect("member should be valid");
        let block = ShaderUniformBlock::new(
            "GlobalUniforms",
            BindingSet::new(0).expect("valid set"),
            BindingIndex::new(1).expect("valid binding"),
            64,
            Box::from([member]),
        )
        .expect("block should be valid");
        let vertex_input = ShaderVertexInput::new(
            "a_Position",
            LocationIndex::new(0).expect("valid location"),
            VertexFormat::R32G32B32Sfloat,
        )
        .expect("vertex input should be valid");
        let reflection = ShaderReflection::new(
            Box::from([uniform_descriptor, image_descriptor, sampler_descriptor]),
            Box::from([block]),
            Box::from([vertex_input]),
            Box::from([TextureSlot::new(0).expect("valid slot")]),
        );

        let json = serde_json::to_string(&reflection).expect("reflection should serialize");

        assert!(json.contains(r#""descriptor":"uniform_buffer""#));
        assert!(json.contains(r#""descriptor":"sampled_image""#));
        assert!(json.contains(r#""descriptor":"sampler""#));
        assert!(json.contains(r#""stages":["vertex","fragment"]"#));
        assert!(json.contains(r#""count":1"#));
        assert!(json.contains(r#""size":64"#));
        assert!(json.contains(r#""element_count":16"#));
        assert!(json.contains(r#""array_count":0"#));
        assert!(json.contains(r#""array_stride":0"#));
        assert!(json.contains(r#""format":"r32g32b32_sfloat""#));
        assert!(!json.contains(r#""kind":"#));
        assert!(!json.contains(r#""byte_size":"#));
    }
}
