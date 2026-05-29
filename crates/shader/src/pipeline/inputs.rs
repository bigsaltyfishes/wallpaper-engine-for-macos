use crate::{
    ShaderError, ShaderResult,
    preprocess::PreprocessedStage,
    syntax::{ParsingContext, ShaderModule, ShaderSourceText},
};

/// Stage-local pipeline inputs.
#[derive(Debug)]
pub struct ProgramStageInputs<'src> {
    /// Parsed stages in request order.
    stages: Vec<ProgramStageInput<'src>>,
}

impl<'src> ProgramStageInputs<'src> {
    /// Constructs paired preprocessed and metadata-preserving stage inputs.
    ///
    /// # Errors
    ///
    /// Returns an error when the stage and metadata-source lists differ in
    /// length, when paired stages have different kinds, or when either source
    /// cannot be parsed.
    #[inline]
    #[allow(clippy::single_call_fn)]
    pub fn new(
        stages: &'src [PreprocessedStage],
        metadata_sources: &'src [PreprocessedStage],
    ) -> ShaderResult<Self> {
        if stages.len() != metadata_sources.len() {
            return Err(ShaderError::invalid_request(
                "preprocessed stage count does not match metadata stage count",
            ));
        }
        let stages = stages
            .iter()
            .zip(metadata_sources)
            .map(|(stage, metadata_stage)| {
                if stage.kind() != metadata_stage.kind() {
                    return Err(ShaderError::invalid_request(
                        "preprocessed stage kind does not match metadata stage kind",
                    ));
                }
                Ok(ProgramStageInput {
                    stage,
                    module: ProgramStageInput::parse(stage)?,
                    metadata_module: ProgramStageInput::parse(metadata_stage)?,
                })
            })
            .collect::<ShaderResult<Vec<_>>>()?;
        Ok(Self { stages })
    }

    /// Parses all preprocessed stages.
    ///
    /// # Errors
    ///
    /// Returns an error when any preprocessed stage cannot be parsed into a
    /// typed syntax module.
    pub fn parse(stages: &'src [PreprocessedStage]) -> ShaderResult<Self> {
        Self::new(stages, stages)
    }

    /// Returns parsed stages.
    #[must_use]
    pub fn stages(&self) -> &[ProgramStageInput<'src>] {
        &self.stages
    }
}

/// One preprocessed stage and its parsed syntax module.
#[derive(Debug)]
pub struct ProgramStageInput<'src> {
    /// Preprocessed stage source.
    pub stage: &'src PreprocessedStage,
    /// Parsed syntax module.
    pub module: ShaderModule<'src>,
    /// Parsed metadata syntax module with includes expanded before condition
    /// stripping.
    pub metadata_module: ShaderModule<'src>,
}

impl ProgramStageInput<'_> {
    /// Parses preprocessed stage source into a typed syntax module.
    fn parse(stage: &PreprocessedStage) -> ShaderResult<ShaderModule<'_>> {
        let context = ParsingContext::new(stage.kind(), ShaderSourceText::new(stage.source()))?;
        context.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShaderStageKind;

    #[test]
    fn parse_paired_rejects_stage_count_mismatch() {
        let stages = [stage(ShaderStageKind::Vertex)];
        let metadata_sources = [];

        let Err(err) = ProgramStageInputs::new(&stages, &metadata_sources) else {
            panic!("mismatched stage counts are rejected");
        };

        assert!(
            err.to_string()
                .contains("preprocessed stage count does not match metadata stage count")
        );
    }

    #[test]
    fn parse_paired_rejects_stage_kind_mismatch() {
        let stages = [stage(ShaderStageKind::Vertex)];
        let metadata_sources = [stage(ShaderStageKind::Fragment)];

        let Err(err) = ProgramStageInputs::new(&stages, &metadata_sources) else {
            panic!("mismatched stage kinds are rejected");
        };

        assert!(
            err.to_string()
                .contains("preprocessed stage kind does not match metadata stage kind")
        );
    }

    fn stage(kind: ShaderStageKind) -> PreprocessedStage {
        PreprocessedStage::new(kind, "void main() {}\n".to_owned())
    }
}
