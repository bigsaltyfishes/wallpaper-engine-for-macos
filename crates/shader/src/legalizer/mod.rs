//! Consolidated Wallpaper Engine shader legalizer.

mod context;
mod declarations;
mod declarators;
mod emission;
mod expressions;
mod fixups;
mod policies;
mod tokens;

pub(crate) use context::LegalizationContext;
pub(crate) use declarations::{
    DeclarationPlan, FragmentOutput, FunctionEntry, InterfaceDirection, SamplerType,
    StageInterfaceInitializer, StageInterfaceLayout, StageInterfaceLayoutBinding,
    StageResourceLayout, SynthesizedName, SynthesizedStageInterface, UniformMember,
};
pub(crate) use declarators::{
    DeclarationDeclarators, DeclaratorInitializer, FunctionParameterQualifier, LocalDeclaration,
    LocalDeclarationStart, LocalTypeName, ScopedDeclarationFacts, ScopedDeclarationFactsConfig,
    ScopedDeclarationTypeMode,
};
pub(crate) use expressions::{ExpressionReplacement, FunctionCall, FunctionCallIndex};
pub(crate) use fixups::{Fixup, FixupSet};
pub(crate) use tokens::{DefineDirectiveTokenExt, StageInputWrite, TokenSearch, TokenView};

use crate::{ShaderDiagnostic, ShaderResult, ShaderStageKind, syntax::ShaderModule};

/// Default shader legalizer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Legalizer;

impl Legalizer {
    /// Legalizes a parsed shader module into backend-accepted GLSL.
    ///
    /// # Errors
    ///
    /// Returns an error when semantic analysis, resource layout planning, or
    /// source emission cannot produce renderer-targeted GLSL.
    pub fn legalize(&self, module: &ShaderModule<'_>) -> ShaderResult<LegalizedStageSource> {
        Self::legalize_with_program_layout(
            module,
            StageInterfaceLayout::default(),
            StageResourceLayout::default(),
        )
    }

    /// Legalizes a parsed shader module with program-level interface and
    /// resource layout information.
    pub(crate) fn legalize_with_program_layout<'src>(
        module: &ShaderModule<'src>,
        interface_layout: StageInterfaceLayout<'src>,
        resource_layout: StageResourceLayout,
    ) -> ShaderResult<LegalizedStageSource> {
        let mut declarations = DeclarationPlan::try_from(module)?;
        declarations.interface_layout = interface_layout;
        declarations.resource_layout = resource_layout;
        LegalizationContext {
            module,
            tokens: TokenView {
                tokens: module.tokens(),
            },
            declarations,
            fixups: FixupSet::default(),
            diagnostics: Vec::new(),
        }
        .legalize()
    }
}

/// Legalized shader source for one stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegalizedStageSource {
    /// Shader stage that owns the emitted source.
    stage: ShaderStageKind,
    /// Complete backend-facing GLSL source.
    source: String,
    /// Diagnostics produced while legalizing this stage.
    diagnostics: Box<[ShaderDiagnostic]>,
}

impl LegalizedStageSource {
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

    /// Returns legalization diagnostics.
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
    fn reverse_policy_order_legalizes_overlapping_rewrites() {
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

        let legalized = LegalizationContext {
            module: &module,
            tokens: TokenView {
                tokens: module.tokens(),
            },
            declarations: DeclarationPlan::try_from(&module).expect("declarations plan"),
            fixups: FixupSet::default(),
            diagnostics: Vec::new(),
        }
        .legalize_with_policy_order(policies::PolicyOrder::Reverse)
        .expect("shader legalizes");
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
}
