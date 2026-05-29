use crate::{
    ShaderCompiler, ShaderMetadata, ShaderReflection, ShaderReflector, ShaderResult,
    ShaderTextureInfo,
    legalize::{LegalizedStageSource, Legalizer, StageInterfaceLayout, StageResourceLayout},
    metadata::ShaderModuleMetadataExt,
    preprocess::PreprocessedStage,
    syntax::{ParsingContext, ShaderModule, ShaderSourceText},
};

/// Stage-local pipeline inputs.
pub(super) struct StagePipeline<'src, 'module, 'backend, C, R> {
    /// Preprocessed stage source.
    pub(super) stage: &'src PreprocessedStage,
    /// Parsed stage module.
    pub(super) module: &'module ParsedStage<'src>,
    /// Parsed stage module used only for legacy metadata extraction.
    pub(super) metadata_module: &'module ParsedStage<'src>,
    /// Program-level interface layout for this stage.
    pub(super) interface_layout: StageInterfaceLayout<'src>,
    /// Program-level resource layout for this stage.
    pub(super) resource_layout: StageResourceLayout<'src>,
    /// Compiler backend.
    pub(super) compiler: &'backend C,
    /// Reflection backend.
    pub(super) reflector: &'backend R,
    /// Request texture metadata.
    pub(super) textures: &'module [ShaderTextureInfo],
}

impl<C, R> StagePipeline<'_, '_, '_, C, R>
where
    C: ShaderCompiler,
    R: ShaderReflector<C::Module>,
{
    /// Parses, extracts metadata, legalizes, compiles, and reflects one stage.
    pub(super) fn compile(self) -> ShaderResult<StageOutput<C::Module>> {
        let metadata = self.metadata_module.extract_metadata(self.textures)?;
        let legalized = Legalizer::legalize_with_program_layout(
            self.module,
            self.interface_layout,
            self.resource_layout,
        )?;
        let artifact = self.compiler.compile_stage(self.stage.kind(), &legalized)?;
        let reflection = self
            .reflector
            .reflect_stage(self.stage.kind(), artifact.module())?;

        Ok(StageOutput {
            metadata,
            legalized,
            artifact,
            reflection,
        })
    }
}

/// Parsed program stages retained so program-level checks can run before
pub(super) struct ParsedStage<'src> {
    /// Parsed shader module.
    module: ShaderModule<'src>,
}

impl<'src> TryFrom<&'src PreprocessedStage> for ParsedStage<'src> {
    type Error = crate::ShaderError;

    /// Parses preprocessed stage source into a typed syntax module.
    fn try_from(stage: &'src PreprocessedStage) -> ShaderResult<Self> {
        let context = ParsingContext::new(stage.kind(), ShaderSourceText::new(stage.source()))?;
        Ok(Self {
            module: context.parse()?,
        })
    }
}

impl<'src> std::ops::Deref for ParsedStage<'src> {
    type Target = ShaderModule<'src>;

    fn deref(&self) -> &Self::Target {
        &self.module
    }
}

/// Stage pipeline output retained until program merge completes.
pub(super) struct StageOutput<M> {
    /// Extracted metadata.
    pub(super) metadata: ShaderMetadata,
    /// Legalized source.
    pub(super) legalized: LegalizedStageSource,
    /// Compiled artifact and backend module.
    pub(super) artifact: crate::CompiledStageArtifact<M>,
    /// Reflected stage metadata.
    pub(super) reflection: ShaderReflection,
}
