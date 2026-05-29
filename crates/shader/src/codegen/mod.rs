//! Consolidated Wallpaper Engine shader legalizer.

mod context;
pub mod declarations;
pub mod declarators;
mod emission;
pub mod expressions;
pub mod fixups;
mod strategies;
mod tokens;

use context::CodegenContext;
pub(crate) use declarations::{
    DeclarationEntry, DeclarationPlan, FragmentOutput, FunctionEntry, InterfaceDirection,
    LegacyTypeName, PlannedDeclarationSource, SamplerType, StageInterfaceInitializer,
    StageInterfaceLayout, StageInterfaceLayoutBinding, StageResourceLayout,
    SynthesizedStageInterface, UniformMember,
};
pub(crate) use declarators::{
    DeclaratorInitializer, FunctionParameterQualifier, LocalDeclaration, ScopedDeclarationFacts,
    ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
};
pub(crate) use expressions::ExpressionReplacement;
pub(crate) use fixups::{Fixup, FixupSet};

use crate::{
    ShaderDiagnostic, ShaderResult, ShaderStageKind,
    syntax::{ShaderModule, SyntaxItem},
};

/// Default shader legalizer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Codegen;

impl Codegen {
    /// Codegens a parsed shader module into backend-accepted GLSL.
    ///
    /// # Errors
    ///
    /// Returns an error when semantic analysis, resource layout planning, or
    /// source emission cannot produce renderer-targeted GLSL.
    pub fn legalize(&self, module: &ShaderModule<'_>) -> ShaderResult<CodegenStageSource> {
        Self::legalize_with_program_layout(
            module,
            StageInterfaceLayout::default(),
            StageResourceLayout::default(),
        )
    }

    /// Codegens a parsed shader module with program-level interface and
    /// resource layout information.
    ///
    /// # Errors
    ///
    /// Returns an error when semantic analysis, resource layout planning, or
    /// source emission cannot produce renderer-targeted GLSL.
    pub(crate) fn legalize_with_program_layout(
        module: &ShaderModule<'_>,
        interface_layout: StageInterfaceLayout,
        resource_layout: StageResourceLayout,
    ) -> ShaderResult<CodegenStageSource> {
        let mut entries = Vec::new();
        let mut functions = Vec::new();

        for item in module.items() {
            match item {
                SyntaxItem::Declaration(declaration) => {
                    entries.push(DeclarationEntry {
                        span: declaration.span(),
                        kind: PlannedDeclarationSource {
                            module,
                            declaration,
                        }
                        .resolve(),
                    });
                }
                SyntaxItem::Function(function) => {
                    functions.push(FunctionEntry {
                        name: function.name(),
                        name_span: function.name_span(),
                    });
                }
                SyntaxItem::Annotation(_) | SyntaxItem::Directive(_) | SyntaxItem::Opaque(_) => {}
            }
        }
        let declarations = DeclarationPlan {
            stage: module.stage(),
            entries,
            functions,
            fragment_output: false,
            compatibility_functions:
                declarations::functions::CompatibilityFunctionRequests::default(),
            interface_layout,
            resource_layout,
        };
        CodegenContext {
            module,
            declarations,
            fixups: FixupSet::default(),
            diagnostics: Vec::new(),
        }
        .legalize()
    }
}

/// Codegen shader source for one stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenStageSource {
    /// Shader stage that owns the emitted source.
    stage: ShaderStageKind,
    /// Complete backend-facing GLSL source.
    source: String,
    /// Diagnostics produced while legalizing this stage.
    diagnostics: Box<[ShaderDiagnostic]>,
}

impl CodegenStageSource {
    /// Creates legalized stage source.
    #[must_use]
    pub fn new(
        stage: ShaderStageKind,
        source: String,
        diagnostics: Box<[ShaderDiagnostic]>,
    ) -> Self {
        Self {
            stage,
            source,
            diagnostics,
        }
    }

    /// Returns the shader stage.
    #[must_use]
    pub const fn stage(&self) -> ShaderStageKind {
        self.stage
    }

    /// Returns legalized GLSL source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns codegen diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[ShaderDiagnostic] {
        &self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ShaderModule;

    #[test]
    fn ordered_pipeline_legalizes_overlapping_rewrites() {
        let source = concat!(
            "float mod(float x, float y) { return x - y; }\n",
            "void main() {\n",
            "    float x = 5.5;\n",
            "    float y = 2.0;\n",
            "    float user_wrapped = mod(x, y);\n",
            "    float builtin_wrapped = x % y;\n",
            "    vec2 color = mix(vec2(0.0), 1, 1);\n",
            "    gl_FragColor = vec4(color, user_wrapped + builtin_wrapped, 1);\n",
            "}\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

        let legalized = Codegen.legalize(&module).expect("shader legalizes");
        let source = legalized.source();

        assert!(source.contains("float _we_user_mod(float x, float y)"));
        assert!(source.contains("float user_wrapped = _we_user_mod(x, y);"));
        assert!(source.contains("float builtin_wrapped = fmod(x, y);"));
        assert!(source.contains("vec2 color = mix(vec2(0.0), vec2(1.0), 1.0);"));
        assert!(source.contains("_we_FragColor = vec4(color, user_wrapped + builtin_wrapped, 1);"));
        assert!(!source.contains("float mod(float x, float y)"));
        assert!(!source.contains("float user_wrapped = mod(x, y);"));
        assert!(!source.contains("float builtin_wrapped = x % y;"));
    }

    #[test]
    fn leading_define_array_suffix_uses_typed_directive_value() {
        let source = concat!(
            "#define LIGHT_COUNT 4 // generated combo\n",
            "uniform vec4 lights[LIGHT_COUNT];\n",
            "void main() { gl_FragColor = lights[0]; }\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

        let legalized = Codegen.legalize(&module).expect("shader legalizes");

        assert!(legalized.source().contains("vec4 lights[4];"));
    }

    #[test]
    fn control_flow_coercion_uses_define_literal_type() {
        let source = concat!(
            "#define ENABLE_LIGHT 1 // generated combo\n",
            "void main() {\n",
            "    if (ENABLE_LIGHT) {\n",
            "        gl_FragColor = vec4(1.0);\n",
            "    }\n",
            "}\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

        let legalized = Codegen.legalize(&module).expect("shader legalizes");

        assert!(legalized.source().contains("if (ENABLE_LIGHT != 0)"));
    }

    #[test]
    fn legalizes_calls_inside_define_replacement_bodies() {
        let source = concat!(
            "#define SAMPLE(tex, uv) tex2D(tex, uv)\n",
            "uniform sampler2D colorMap;\n",
            "varying vec2 uv;\n",
            "void main() { gl_FragColor = SAMPLE(colorMap, uv); }\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

        let legalized = Codegen.legalize(&module).expect("shader legalizes");

        assert!(
            legalized
                .source()
                .contains("#define SAMPLE(tex, uv) texture(")
        );
    }
}
