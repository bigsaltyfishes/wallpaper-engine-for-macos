use shader::{
    ShaderStageKind,
    syntax::{
        AnnotationKind, DeclarationArraySize, DeclarationKind, ParsingContext, ShaderModule,
        SyntaxItem, TopLevelQualifier,
    },
    tokenizer::{KeywordType, LiteralValue, PrimitiveType, TokenStream, TypedToken},
};

#[test]
fn token_stream_exposes_vec_like_read_access_for_api_migration() {
    let source = "uniform float value;\n";
    let tokens = TokenStream::lex(source).expect("shader should lex");

    assert_eq!(tokens.len(), tokens.as_slice().len());
    assert_eq!(tokens.iter().count(), tokens.len());
    assert_eq!(tokens.as_ref(), tokens.as_slice());
}

#[test]
fn token_stream_searches_comments_and_balanced_delimiters() {
    let source = "void main /* c */ () { color[0] = vec4((1.0)); }\n";
    let stream = TokenStream::lex(source).expect("shader should lex");
    let tokens = stream.as_slice();
    let name = tokens
        .iter()
        .position(|token| matches!(token.kind(), TypedToken::Identifier(name) if name == "main"))
        .expect("main token exists");
    let paren = tokens
        .iter()
        .position(|token| token.kind().is_left_paren())
        .expect("left paren exists");
    let brace = tokens
        .iter()
        .position(|token| matches!(token.kind(), TypedToken::LeftBrace))
        .expect("left brace exists");
    let bracket = tokens
        .iter()
        .position(|token| token.kind().is_left_square())
        .expect("left bracket exists");

    assert_eq!(stream.next_non_comment(name + 1), Some(paren));
    assert_eq!(stream.previous_non_comment(paren), Some(name));
    assert!(matches!(
        tokens[stream
            .matching_right_paren(paren)
            .expect("right paren matches")]
        .kind(),
        kind if kind.is_right_paren()
    ));
    assert!(matches!(
        tokens[stream
            .matching_right_brace(brace)
            .expect("right brace matches")]
        .kind(),
        TypedToken::RightBrace
    ));
    assert!(matches!(
        tokens[stream
            .matching_right_square(bracket)
            .expect("right bracket matches")]
        .kind(),
        kind if kind.is_right_square()
    ));
}

#[test]
fn token_stream_searches_matches_and_reports_identifiers() {
    let source = "void main /* c */ () { float color[2]; }\n";
    let stream = TokenStream::lex(source).expect("shader should lex");
    let tokens = stream.as_slice();
    let main = tokens
        .iter()
        .position(|token| matches!(token.kind(), TypedToken::Identifier(name) if name == "main"))
        .expect("main token exists");
    let paren = tokens
        .iter()
        .position(|token| token.kind().is_left_paren())
        .expect("left paren exists");
    let brace = tokens
        .iter()
        .position(|token| matches!(token.kind(), TypedToken::LeftBrace))
        .expect("left brace exists");
    let bracket = tokens
        .iter()
        .position(|token| token.kind().is_left_square())
        .expect("left bracket exists");

    assert_eq!(stream.next_non_comment(main + 1), Some(paren));
    assert_eq!(stream.previous_non_comment(paren), Some(main));
    assert!(matches!(
        tokens[stream
            .matching_right_paren(paren)
            .expect("right paren matches")]
        .kind(),
        kind if kind.is_right_paren()
    ));
    assert!(matches!(
        tokens[stream
            .matching_right_brace(brace)
            .expect("right brace matches")]
        .kind(),
        TypedToken::RightBrace
    ));
    assert!(matches!(
        tokens[stream
            .matching_right_square(bracket)
            .expect("right bracket matches")]
        .kind(),
        kind if kind.is_right_square()
    ));

    let identifiers: Vec<_> = stream
        .identifiers()
        .map(|identifier| (identifier.text(), identifier.span()))
        .collect();

    assert_eq!(
        identifiers
            .iter()
            .map(|(text, _)| *text)
            .collect::<Vec<_>>(),
        vec!["void", "main", "float", "color"]
    );
    assert_eq!(
        &source[identifiers[3].1.start()..identifiers[3].1.end()],
        "color"
    );
}

#[test]
fn owned_token_stream_into_iterator_preserves_vec_alias_semantics() {
    let source = "uniform float value;\n";
    let tokens = TokenStream::lex(source).expect("shader should lex");

    let token_kinds: Vec<_> = tokens
        .into_iter()
        .map(|token| token.kind().clone())
        .collect();

    assert_eq!(
        token_kinds,
        vec![
            TypedToken::Keyword(KeywordType::Uniform),
            TypedToken::TypeMark(PrimitiveType::Float),
            TypedToken::Identifier("value".into()),
            TypedToken::Semicolon,
        ]
    );
}

#[test]
fn lexes_wallpaper_engine_annotations() {
    let source = concat!(
        "// [COMBO] {\"combo\":\"ENABLE_BLUR\",\"default\":0}\n",
        "// {\"material\":\"glass\",\"default\":0.5}\n",
        "void main() {}\n",
    );

    let tokens = TokenStream::lex(source).expect("shader should lex");

    let annotation_texts: Vec<&str> = tokens
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Annotation(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        annotation_texts,
        vec![
            "// [COMBO] {\"combo\":\"ENABLE_BLUR\",\"default\":0}",
            "// {\"material\":\"glass\",\"default\":0.5}"
        ]
    );
    assert_eq!(annotation_texts[0], &source[0..annotation_texts[0].len()]);
}

#[test]
fn typed_token_owns_identifier_and_modifier_classification() {
    assert_eq!(
        TypedToken::from_identifier_text("sampler2D"),
        TypedToken::TypeMark(PrimitiveType::Sampler2D)
    );
    assert_eq!(
        TypedToken::Literal(LiteralValue::Number("1.0".into())).identifier_text(),
        None
    );
    assert!(TypedToken::Identifier("highp".into()).is_declaration_modifier());
    assert!(!TypedToken::Keyword(KeywordType::Uniform).is_declaration_modifier());
}

#[test]
fn parses_wallpaper_engine_annotations_as_syntax_items() {
    let source = concat!(
        "// [COMBO] {\"combo\":\"BLENDMODE\",\"default\":0}\n",
        "// {\"material\":\"glass\",\"default\":0.5}\n",
        "uniform vec4 g_Color;\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

    assert_eq!(module.stage(), ShaderStageKind::Fragment);
    assert_eq!(module.items().len(), 3);

    let SyntaxItem::Annotation(combo) = &module.items()[0] else {
        panic!("first item should be a combo annotation");
    };
    assert_eq!(combo.kind(), AnnotationKind::Combo);
    assert_eq!(
        combo.text_in(&module),
        "// [COMBO] {\"combo\":\"BLENDMODE\",\"default\":0}"
    );

    let SyntaxItem::Annotation(metadata) = &module.items()[1] else {
        panic!("second item should be a JSON annotation");
    };
    assert_eq!(metadata.kind(), AnnotationKind::Json);
    assert_eq!(
        metadata.text_in(&module),
        "// {\"material\":\"glass\",\"default\":0.5}"
    );
}

#[test]
fn parsing_context_owns_source_tokens_and_typed_slicing() {
    let source = concat!(
        "// [COMBO] {\"combo\":\"BLENDMODE\",\"default\":0}\n",
        "uniform sampler2D g_Texture0;\n",
        "void main() { gl_FragColor = texture2D(g_Texture0, vec2(0.5)); }\n",
    );

    let context =
        ParsingContext::from_str(ShaderStageKind::Fragment, source).expect("context lexes");
    let module = context.parse().expect("module parses");

    assert_eq!(context.stage(), ShaderStageKind::Fragment);
    assert_eq!(module.stage(), ShaderStageKind::Fragment);
    assert!(matches!(
        context
            .token_stream()
            .as_slice()
            .first()
            .map(|token| token.kind()),
        Some(TypedToken::Annotation(_))
    ));

    let SyntaxItem::Annotation(combo) = &module.items()[0] else {
        panic!("first item should be a combo annotation");
    };
    assert_eq!(
        combo.text_in(&module),
        "// [COMBO] {\"combo\":\"BLENDMODE\",\"default\":0}"
    );

    let SyntaxItem::Declaration(declaration) = &module.items()[1] else {
        panic!("second item should be a declaration");
    };
    assert_eq!(
        declaration.text_in(&module),
        "uniform sampler2D g_Texture0;"
    );

    let SyntaxItem::Function(function) = &module.items()[2] else {
        panic!("third item should be a function");
    };
    assert_eq!(function.parameters_in(&module), "");
    assert!(function.body_in(&module).starts_with("{ gl_FragColor"));
}

#[test]
fn lexes_preprocessor_directives_with_spans() {
    let source = "#define LIGHT_COUNT 4\n#include \"common.glsl\"\n";

    let tokens = TokenStream::lex(source).expect("shader should lex");
    let directives: Vec<_> = tokens
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Directive(text) => Some((text.as_str(), token.span())),
            _ => None,
        })
        .collect();

    assert_eq!(directives.len(), 2);
    assert_eq!(directives[0].0, "#define LIGHT_COUNT 4");
    assert_eq!(directives[0].1.start(), 0);
    assert_eq!(directives[0].1.end(), "#define LIGHT_COUNT 4".len());
    assert_eq!(directives[1].0, "#include \"common.glsl\"");
    assert_eq!(
        &source[directives[1].1.start()..directives[1].1.end()],
        "#include \"common.glsl\""
    );
}

#[test]
fn parses_typed_directives_without_changing_token_spans() {
    let source = concat!(
        "#define LIGHT_COUNT 4\n",
        "#include \"common.glsl\"\n",
        "#if LIGHT_COUNT == 4\n",
        "#endif\n",
    );

    let tokens = TokenStream::lex(source).expect("shader should lex");
    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let token_spans: Vec<_> = tokens
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Directive(_) => Some(token.span()),
            _ => None,
        })
        .collect();
    let directives: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Directive(directive) => Some(directive),
            _ => None,
        })
        .collect();

    assert_eq!(directives.len(), token_spans.len());
    assert_eq!(
        directives
            .iter()
            .map(|directive| directive.span())
            .collect::<Vec<_>>(),
        token_spans
    );
    assert!(directives[0].kind().is_define());
    assert_eq!(directives[0].kind().name().as_str(), "define");
    assert_eq!(directives[0].kind().body().as_str(), "LIGHT_COUNT 4");
    assert!(directives[1].kind().is_include());
    assert_eq!(
        directives[1].kind().body().include_path_text(),
        Some("common.glsl")
    );
    assert!(directives[2].kind().is_conditional());
    assert_eq!(directives[2].kind().name().as_str(), "if");
    assert_eq!(directives[2].kind().body().as_str(), "LIGHT_COUNT == 4");
}

#[test]
fn parses_top_level_declarations_and_structs() {
    let source = concat!(
        "uniform sampler2D g_Texture0;\n",
        "attribute vec3 a_Position;\n",
        "varying vec2 v_TexCoord;\n",
        "in vec4 a_Color;\n",
        "out vec4 o_Color;\n",
        "struct Material {\n",
        "    vec4 tint;\n",
        "    float roughness;\n",
        "};\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Vertex, source).expect("module parses");
    let declarations: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Declaration(declaration) => Some(declaration),
            _ => None,
        })
        .collect();

    assert_eq!(declarations.len(), 6);
    assert_eq!(
        declarations[0].qualifier(),
        Some(TopLevelQualifier::Uniform)
    );
    assert_eq!(declarations[0].type_name(), Some("sampler2D"));
    assert_eq!(declarations[0].name(), Some("g_Texture0"));
    assert_eq!(
        declarations[1].qualifier(),
        Some(TopLevelQualifier::Attribute)
    );
    assert_eq!(
        declarations[2].qualifier(),
        Some(TopLevelQualifier::Varying)
    );
    assert_eq!(declarations[3].qualifier(), Some(TopLevelQualifier::In));
    assert_eq!(declarations[4].qualifier(), Some(TopLevelQualifier::Out));
    assert_eq!(declarations[5].kind(), DeclarationKind::Struct);
    assert_eq!(declarations[5].name(), Some("Material"));
}

#[test]
fn parses_function_signatures_and_balanced_body_spans() {
    let source = concat!(
        "float helper(float x) {\n",
        "    if (x > 0.0) {\n",
        "        return x;\n",
        "    }\n",
        "    return 0.0;\n",
        "}\n",
        "void main() { gl_FragColor = vec4(helper(1.0)); }\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let functions: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        })
        .collect();

    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].return_type(), "float");
    assert_eq!(functions[0].name(), "helper");
    assert_eq!(module.slice(functions[0].name_span()), "helper");
    assert_eq!(functions[0].parameters_in(&module), "float x");
    assert!(functions[0].body_in(&module).starts_with("{\n    if"));
    assert!(functions[0].body_in(&module).ends_with('}'));
    assert_eq!(functions[1].return_type(), "void");
    assert_eq!(functions[1].name(), "main");
}

#[test]
fn parses_function_signatures_with_interleaved_comments() {
    let source = concat!(
        "float /* c */ helper(float x) {}\n",
        "void main() /* before body */ { helper(1.0); }\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let functions: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        })
        .collect();

    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].return_type(), "float");
    assert_eq!(functions[0].name(), "helper");
    assert_eq!(functions[1].return_type(), "void");
    assert_eq!(functions[1].name(), "main");
    assert_eq!(functions[1].body_in(&module), "{ helper(1.0); }");
}

#[test]
fn allows_user_defined_function_named_mod() {
    let source = concat!(
        "float mod(float value, float divisor) {\n",
        "    return value - divisor;\n",
        "}\n",
        "void main() { float x = mod(4.0, 2.0); }\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let names: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function.name()),
            _ => None,
        })
        .collect();

    assert_eq!(names, vec!["mod", "main"]);
}

#[test]
fn parses_declarations_with_precision_and_layout_qualifiers() {
    let source = concat!(
        "uniform highp sampler2D tex;\n",
        "layout(location=0) out vec4 color;\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
    let declarations: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Declaration(declaration) => Some(declaration),
            _ => None,
        })
        .collect();

    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations[0].qualifier(),
        Some(TopLevelQualifier::Uniform)
    );
    assert_eq!(declarations[0].type_name(), Some("sampler2D"));
    assert_eq!(declarations[0].name(), Some("tex"));
    assert_eq!(declarations[1].qualifier(), Some(TopLevelQualifier::Out));
    assert_eq!(declarations[1].type_name(), Some("vec4"));
    assert_eq!(declarations[1].name(), Some("color"));
}

#[test]
fn parses_declaration_layout_binding_and_array_size_facts() {
    let source = concat!(
        "#define LOCAL_COUNT 4\n",
        "layout(binding = 5) layout(std140) uniform vec4 offsets[LOCAL_COUNT];\n",
        "layout(set = 1, binding = 2) uniform samplerCube env;\n",
    );

    let module = ShaderModule::parse(ShaderStageKind::Vertex, source).expect("module parses");
    let declarations: Vec<_> = module
        .items()
        .iter()
        .filter_map(|item| match item {
            SyntaxItem::Declaration(declaration) => Some(declaration),
            _ => None,
        })
        .collect();

    let offsets_layout = declarations[0].layout().expect("offsets layout parsed");
    assert_eq!(offsets_layout.binding(), Some(5));
    assert_eq!(offsets_layout.set(), None);
    assert_eq!(
        declarations[0]
            .array_suffix()
            .and_then(|suffix| suffix.size()),
        Some(DeclarationArraySize::MacroIdentifier("LOCAL_COUNT".into()))
    );

    let env_layout = declarations[1].layout().expect("env layout parsed");
    assert_eq!(env_layout.binding(), Some(2));
    assert_eq!(env_layout.set(), Some(1));
    assert_eq!(declarations[1].array_suffix(), None);
}

#[test]
fn lexes_multiline_preprocessor_continuation_as_one_directive() {
    let source = "#define X(a) \\\n  ((a) + 1)\nuniform float value;\n";

    let tokens = TokenStream::lex(source).expect("shader should lex");
    let directives: Vec<_> = tokens
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Directive(text) => Some((text.as_str(), token.span())),
            _ => None,
        })
        .collect();

    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].0, "#define X(a) \\\n  ((a) + 1)");
    assert_eq!(
        &source[directives[0].1.start()..directives[0].1.end()],
        "#define X(a) \\\n  ((a) + 1)"
    );
}

#[test]
fn unbalanced_delimiters_return_parse_error() {
    let source = "void main() { if (true) { gl_FragColor = vec4(1.0); }\n";

    let error = ShaderModule::parse(ShaderStageKind::Fragment, source)
        .expect_err("unbalanced body should fail");

    assert_eq!(error.to_string(), "shader parse failed");
}
