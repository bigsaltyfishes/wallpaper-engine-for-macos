//! Preprocessed program output.

use super::PreprocessedStage;
use crate::ShaderStageKind;

/// Preprocessed shader program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreprocessedProgram {
    /// Preprocessed stage outputs.
    stages: Box<[PreprocessedStage]>,
}

impl PreprocessedProgram {
    /// Creates a preprocessed program from stage outputs.
    #[must_use]
    pub fn new(stages: Box<[PreprocessedStage]>) -> Self {
        Self { stages }
    }

    /// Returns all preprocessed stages.
    #[must_use]
    pub fn stages(&self) -> &[PreprocessedStage] {
        &self.stages
    }

    /// Returns a preprocessed stage by kind.
    #[must_use]
    pub fn stage(&self, kind: ShaderStageKind) -> Option<&PreprocessedStage> {
        self.stages.iter().find(|stage| stage.kind() == kind)
    }
}
