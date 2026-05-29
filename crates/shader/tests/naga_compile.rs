use shader::{
    ShaderCompiler, ShaderError, ShaderStageKind,
    compile::NagaCompiler,
    legalize::{LegalizedStageSource, Legalizer},
    syntax::ShaderModule,
};

const SPIRV_MAGIC: u32 = 0x0723_0203;
const SPIRV_OP_FNEGATE: u16 = 127;

#[test]
fn compiles_legalized_vertex_shader_to_spirv() {
    let source_text = r"#version 450
layout(location = 0) in vec2 a_position;
void main() {
    gl_Position = vec4(a_position, 0.0, 1.0);
}
";
    let source = legalized_source(ShaderStageKind::Vertex, source_text);

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &source)
        .expect("legalized vertex shader should compile");

    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
    assert_eq!(artifact.stage().kind(), ShaderStageKind::Vertex);
    assert_eq!(artifact.stage().spirv().first(), Some(&SPIRV_MAGIC));
    assert_eq!(artifact.stage().legalized_source(), Some(source_text));
    assert_eq!(artifact.module().entry_points.len(), 1);
    assert!(artifact.diagnostics().is_empty());
    assert!(artifact.stage().diagnostics().is_empty());
}

#[test]
fn vertex_spirv_does_not_inject_coordinate_space_y_flip() {
    let source_text = r"#version 450
layout(location = 0) in vec2 a_position;
void main() {
    gl_Position = vec4(a_position, 0.0, 1.0);
}
";
    let source = legalized_source(ShaderStageKind::Vertex, source_text);

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &source)
        .expect("legalized vertex shader should compile");

    assert!(
        !spirv_contains_opcode(artifact.stage().spirv(), SPIRV_OP_FNEGATE),
        "vertex SPIR-V must not contain Naga's BuiltIn::Position coordinate-space Y flip"
    );
}

#[test]
fn compiles_legacy_vertex_shader_with_array_varying_output() {
    let source_text = r"attribute vec3 a_Position;
attribute vec2 a_TexCoord;
uniform vec2 g_TexelSize;
varying vec2 v_TexCoord[4];
void main() {
    gl_Position = vec4(a_Position, 1.0);
    v_TexCoord[0] = a_TexCoord - g_TexelSize;
    v_TexCoord[1] = a_TexCoord + g_TexelSize;
    v_TexCoord[2] = a_TexCoord + vec2(-g_TexelSize.x, g_TexelSize.y);
    v_TexCoord[3] = a_TexCoord + vec2(g_TexelSize.x, -g_TexelSize.y);
}
";
    let module = ShaderModule::parse(ShaderStageKind::Vertex, source_text).expect("module parses");
    let source = Legalizer
        .legalize(&module)
        .expect("array varying shader legalizes");

    assert!(
        source
            .source()
            .contains("layout(location = 0) out vec2 v_TexCoord[4];")
    );

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Vertex, &source)
        .expect("legalized bloom vertex shader with array varying should compile");

    assert_eq!(artifact.kind(), ShaderStageKind::Vertex);
    assert_eq!(artifact.module().entry_points.len(), 1);
}

#[test]
fn compiles_legalized_fragment_shader_to_spirv() {
    let source_text = r"#version 450
layout(location = 0) out vec4 frag_color;
void main() {
    frag_color = vec4(1.0, 0.0, 0.0, 1.0);
}
";
    let source = legalized_source(ShaderStageKind::Fragment, source_text);

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &source)
        .expect("legalized fragment shader should compile");

    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
    assert_eq!(artifact.stage().kind(), ShaderStageKind::Fragment);
    assert_eq!(artifact.stage().spirv().first(), Some(&SPIRV_MAGIC));
    assert_eq!(artifact.stage().legalized_source(), Some(source_text));
    assert_eq!(artifact.module().entry_points.len(), 1);
    assert!(artifact.diagnostics().is_empty());
    assert!(artifact.stage().diagnostics().is_empty());
}

#[test]
fn compiles_legacy_fragment_with_scalar_expression_vector_max() {
    let source_text = r"float luma(vec3 color) {
    return dot(color, vec3(0.299, 0.587, 0.114));
}
void main() {
    vec3 color = vec3(1.25, 1.0, 0.75);
    color += max(luma(color) - 1.0, vec3(0.0));
    gl_FragColor = vec4(color, 1.0);
}
";
    let module =
        ShaderModule::parse(ShaderStageKind::Fragment, source_text).expect("module parses");
    let source = Legalizer
        .legalize(&module)
        .expect("scalar expression vector max shader legalizes");

    assert!(
        source
            .source()
            .contains("color += max(vec3(luma(color) - 1.0), vec3(0.0));")
    );

    let artifact = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &source)
        .expect("scalar expression vector max shader should compile");

    assert_eq!(artifact.kind(), ShaderStageKind::Fragment);
    assert_eq!(artifact.module().entry_points.len(), 1);
}

#[test]
fn failure_diagnostics_preserve_stage_path_and_source_snippet() {
    let source_text = r"#version 450
layout(location = 0) out vec4 frag_color;
void main() {
    frag_color = missing_value;
}
";
    let source = legalized_source(ShaderStageKind::Fragment, source_text);

    let err = NagaCompiler
        .compile_stage(ShaderStageKind::Fragment, &source)
        .expect_err("unknown GLSL symbol should fail compilation");

    let ShaderError::Compile { diagnostics } = err else {
        panic!("expected compile diagnostics");
    };
    let diagnostic = diagnostics
        .first()
        .expect("compile failure should contain a diagnostic");

    assert_eq!(diagnostic.stage(), Some(ShaderStageKind::Fragment));
    assert_eq!(diagnostic.pass(), Some("naga glsl parse"));
    assert_eq!(
        diagnostic.generated_source_path(),
        Some("generated/fragment.glsl")
    );
    assert!(diagnostic.message().contains("missing_value"));
    assert!(diagnostic.message().contains("generated/fragment.glsl"));
    assert!(diagnostic.message().contains("frag_color = missing_value;"));
    assert!(diagnostic.span().is_some());
}

fn legalized_source(stage: ShaderStageKind, source: &str) -> LegalizedStageSource {
    LegalizedStageSource::new(stage, source.to_owned(), Box::from([]))
}

fn spirv_contains_opcode(words: &[u32], opcode: u16) -> bool {
    let mut index = 5;
    while index < words.len() {
        let instruction = words[index];
        let word_count = (instruction >> 16) as usize;
        let instruction_opcode = (instruction & 0xffff) as u16;
        if instruction_opcode == opcode {
            return true;
        }
        if word_count == 0 {
            return false;
        }
        index += word_count;
    }
    false
}
