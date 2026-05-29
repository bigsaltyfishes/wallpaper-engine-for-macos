use crate::{
    CompiledShaderProgram, ShaderCompiler, ShaderProgramRequest, ShaderReflector, ShaderResult,
    ShaderSourceProvider,
    compile::NagaCompiler,
    metadata::ShaderModuleMetadataExt,
    pipeline::{
        cache::{CacheKeyBuilder, CacheKeySeed},
        inputs::{ProgramStageInputs, ProgramStageSources},
        interface::{ProgramInterface, StageGlobalNames},
        metadata::{MetadataMerger, RequestWithMetadataCombos},
        reflection::ReflectionMerger,
        resources::ProgramResourceLayout,
        revision::{COMPILER_OPTIONS_CACHE_SALT, ShaderPipelineRevision},
        stage::StagePipeline,
    },
    preprocess::PreprocessContext,
    reflect::NagaReflector,
};

pub type DefaultShaderPipeline<P> = ShaderPipeline<P, NagaCompiler, NagaReflector>;

#[derive(Clone, Debug)]
pub struct ShaderPipeline<P, C, R = NagaReflector> {
    /// Source provider used for include expansion.
    provider: P,
    /// Shader compiler backend.
    compiler: C,
    /// Shader reflector backend.
    reflector: R,
    /// Pipeline revision included in cache keys.
    revision: ShaderPipelineRevision,
}

impl<P, C> ShaderPipeline<P, C, NagaReflector> {
    /// Creates a pipeline with the default Naga reflector.
    #[must_use]
    pub const fn new(provider: P, compiler: C) -> Self {
        Self {
            provider,
            compiler,
            reflector: NagaReflector,
            revision: ShaderPipelineRevision::CURRENT,
        }
    }
}

impl<P, C, R> ShaderPipeline<P, C, R> {
    /// Returns the compiler-options identity included in cache keys.
    #[must_use]
    pub const fn compiler_options_cache_salt() -> &'static str {
        COMPILER_OPTIONS_CACHE_SALT
    }

    /// Creates a pipeline with an explicit reflector.
    #[must_use]
    pub const fn with_reflector(provider: P, compiler: C, reflector: R) -> Self {
        Self {
            provider,
            compiler,
            reflector,
            revision: ShaderPipelineRevision::CURRENT,
        }
    }

    /// Returns this pipeline with a different cache revision.
    #[must_use]
    pub const fn with_revision(mut self, revision: ShaderPipelineRevision) -> Self {
        self.revision = revision;
        self
    }
}

impl<P, C, R> ShaderPipeline<P, C, R>
where
    P: ShaderSourceProvider,
    C: ShaderCompiler,
    R: ShaderReflector<C::Module>,
{
    /// Compiles a full shader program request.
    ///
    /// # Errors
    ///
    /// Returns an error when preprocessing, parsing, metadata extraction,
    /// legalization, compilation, or reflection fails.
    pub fn compile(&self, request: &ShaderProgramRequest) -> ShaderResult<CompiledShaderProgram> {
        PipelineContext {
            pipeline: self,
            request,
        }
        .compile()
    }
}

/// Borrowed context for compiling one request through a pipeline.
pub(super) struct PipelineContext<'pipeline, 'request, P, C, R> {
    /// Pipeline components.
    pipeline: &'pipeline ShaderPipeline<P, C, R>,
    /// Request being compiled.
    request: &'request ShaderProgramRequest,
}

impl<P, C, R> PipelineContext<'_, '_, P, C, R>
where
    P: ShaderSourceProvider,
    C: ShaderCompiler,
    R: ShaderReflector<C::Module>,
{
    /// Runs the full pipeline and builds the public program output.
    fn compile(&self) -> ShaderResult<CompiledShaderProgram> {
        let request_with_metadata_combos =
            self.request_with_annotation_combo_defaults(&self.pipeline.provider)?;
        let request = request_with_metadata_combos
            .as_ref()
            .unwrap_or(self.request);
        let preprocess_context = PreprocessContext::new(request, &self.pipeline.provider);
        let metadata_sources = preprocess_context.expand_includes_preserving_conditionals()?;
        let preprocessed = preprocess_context.preprocess()?;
        let stage_inputs = ProgramStageInputs::try_from(ProgramStageSources {
            stages: preprocessed.stages(),
            metadata_sources: metadata_sources.stages(),
        })?;
        let stage_global_names = StageGlobalNames::from(stage_inputs.stages());
        let program_interface = ProgramInterface::from(stage_inputs.stages())
            .validate_with_names(&stage_global_names)?;
        let program_resources = ProgramResourceLayout::from_inputs(&stage_inputs)?;

        let mut stages = Vec::with_capacity(preprocessed.stages().len());
        let mut metadata = MetadataMerger::default();
        let mut reflection = ReflectionMerger::default();
        let mut diagnostics = Vec::new();
        let mut cache_builder = CacheKeyBuilder::from(CacheKeySeed {
            revision: self.pipeline.revision,
            request,
        });

        for input in stage_inputs.stages() {
            let stage_output = StagePipeline {
                stage: input.stage,
                module: &input.module,
                metadata_module: &input.metadata_module,
                interface_layout: program_interface.layout_for_stage(input.stage.kind()),
                resource_layout: program_resources.stage_layout(),
                compiler: &self.pipeline.compiler,
                reflector: &self.pipeline.reflector,
                textures: request.textures(),
            }
            .compile()?;

            metadata.push(&stage_output.metadata);
            reflection.push(&stage_output.reflection)?;
            diagnostics.extend_from_slice(stage_output.legalized.diagnostics());
            diagnostics.extend_from_slice(stage_output.artifact.diagnostics());
            diagnostics.extend_from_slice(stage_output.artifact.stage().diagnostics());
            cache_builder.push_stage(input.stage, &stage_output.legalized);
            stages.push(stage_output.artifact.stage().clone());
        }

        let reflection = reflection.finish();
        let active_texture_slots = reflection
            .active_texture_slots()
            .to_vec()
            .into_boxed_slice();
        let metadata = metadata
            .finish()
            .with_active_texture_slots(active_texture_slots);
        let cache_key = cache_builder.finish();

        Ok(CompiledShaderProgram::with_pipeline_outputs(
            self.request.shader_name().clone(),
            cache_key,
            stages.into_boxed_slice(),
            metadata,
            reflection,
            diagnostics.into_boxed_slice(),
        ))
    }

    /// Returns a request extended with combo defaults discovered in shader
    /// annotations before compile-time macro preprocessing.
    fn request_with_annotation_combo_defaults(
        &self,
        provider: &P,
    ) -> ShaderResult<Option<ShaderProgramRequest>> {
        let preprocess_context = PreprocessContext::new(self.request, provider);
        let stages = preprocess_context.preprocess()?;
        let stage_inputs = ProgramStageInputs::try_from(stages.stages())?;
        let mut metadata = MetadataMerger::default();

        for input in stage_inputs.stages() {
            metadata.push(
                &input
                    .metadata_module
                    .extract_metadata(self.request.textures())?,
            );
        }

        let metadata = metadata.finish();
        let mut builder = RequestWithMetadataCombos::from(self.request);
        for combo in metadata.combos() {
            builder.push_default(combo)?;
        }
        builder.finish()
    }
}
