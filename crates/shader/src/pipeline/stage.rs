use crate::{
    ShaderCompiler, ShaderMetadata, ShaderReflection, ShaderReflector, ShaderResult,
    ShaderTextureInfo,
    legalize::{Codegen, CodegenStageSource, StageInterfaceLayout, StageResourceLayout},
    preprocess::PreprocessedStage,
    syntax::ShaderModule,
};

/// Stage-local pipeline inputs.
pub(super) struct StagePipeline<'src, 'module, 'backend, C, R> {
    /// Preprocessed stage source.
    pub stage: &'src PreprocessedStage,
    /// Parsed stage module.
    pub module: &'module ShaderModule<'src>,
    /// Parsed stage module used only for legacy metadata extraction.
    pub metadata_module: &'module ShaderModule<'src>,
    /// Program-level interface layout for this stage.
    pub interface_layout: StageInterfaceLayout,
    /// Program-level resource layout for this stage.
    pub resource_layout: StageResourceLayout,
    /// Compiler backend.
    pub compiler: &'backend C,
    /// Reflection backend.
    pub reflector: &'backend R,
    /// Request texture metadata.
    pub textures: &'module [ShaderTextureInfo],
}

impl<C, R> StagePipeline<'_, '_, '_, C, R>
where
    C: ShaderCompiler,
    R: ShaderReflector<C::Module>,
{
    /// Parses, extracts metadata, legalizes, compiles, and reflects one stage.
    pub(super) fn compile(self) -> ShaderResult<StageOutput<C::Module>> {
        let metadata = self.metadata_module.extract_metadata(self.textures)?;
        let legalized = Codegen::legalize_with_program_layout(
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

/// Stage pipeline output retained until program merge completes.
pub(super) struct StageOutput<M> {
    /// Extracted metadata.
    pub metadata: ShaderMetadata,
    /// Codegen source.
    pub legalized: CodegenStageSource,
    /// Compiled artifact and backend module.
    pub artifact: crate::CompiledStageArtifact<M>,
    /// Reflected stage metadata.
    pub reflection: ShaderReflection,
}
