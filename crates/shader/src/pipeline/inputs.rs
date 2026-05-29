use crate::{
    ShaderError, ShaderResult, pipeline::stage::ParsedStage, preprocess::PreprocessedStage,
};

/// Stage-local pipeline inputs.
pub(super) struct ProgramStageInputs<'src> {
    /// Parsed stages in request order.
    stages: Vec<ProgramStageInput<'src>>,
}

/// Preprocessed source slices for normal compilation and metadata extraction.
pub(super) struct ProgramStageSources<'src> {
    /// Sources with conditionals evaluated for compilation.
    pub(super) stages: &'src [PreprocessedStage],
    /// Sources preserving conditionals for legacy metadata extraction.
    pub(super) metadata_sources: &'src [PreprocessedStage],
}

impl<'src> TryFrom<&'src [PreprocessedStage]> for ProgramStageInputs<'src> {
    type Error = ShaderError;

    /// Parses all preprocessed stages.
    fn try_from(stages: &'src [PreprocessedStage]) -> ShaderResult<Self> {
        let stages = stages
            .iter()
            .map(|stage| {
                Ok(ProgramStageInput {
                    stage,
                    module: ParsedStage::try_from(stage)?,
                    metadata_module: ParsedStage::try_from(stage)?,
                })
            })
            .collect::<ShaderResult<Vec<_>>>()?;
        Ok(Self { stages })
    }
}

impl<'src> TryFrom<ProgramStageSources<'src>> for ProgramStageInputs<'src> {
    type Error = ShaderError;

    /// Parses preprocessed stages and separate metadata source stages.
    fn try_from(sources: ProgramStageSources<'src>) -> ShaderResult<Self> {
        if sources.stages.len() != sources.metadata_sources.len() {
            return Err(ShaderError::invalid_request(
                "preprocessed stage count does not match metadata stage count",
            ));
        }
        let stages = sources
            .stages
            .iter()
            .zip(sources.metadata_sources)
            .map(|(stage, metadata_stage)| {
                if stage.kind() != metadata_stage.kind() {
                    return Err(ShaderError::invalid_request(
                        "preprocessed stage kind does not match metadata stage kind",
                    ));
                }
                Ok(ProgramStageInput {
                    stage,
                    module: ParsedStage::try_from(stage)?,
                    metadata_module: ParsedStage::try_from(metadata_stage)?,
                })
            })
            .collect::<ShaderResult<Vec<_>>>()?;
        Ok(Self { stages })
    }
}

impl<'src> ProgramStageInputs<'src> {
    /// Returns parsed stages.
    pub(super) fn stages(&self) -> &[ProgramStageInput<'src>] {
        &self.stages
    }
}

/// One preprocessed stage and its parsed syntax module.
pub(super) struct ProgramStageInput<'src> {
    /// Preprocessed stage source.
    pub(super) stage: &'src PreprocessedStage,
    /// Parsed syntax module.
    pub(super) module: ParsedStage<'src>,
    /// Parsed metadata syntax module with includes expanded before condition
    /// stripping.
    pub(super) metadata_module: ParsedStage<'src>,
}
