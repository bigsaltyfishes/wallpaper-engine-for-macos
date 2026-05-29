//! Public preprocessing context and compatibility entry point.

use super::{
    ConditionalMode, MacroPrelude, MacroTable, PreprocessedProgram, PreprocessedStage,
    StagePreprocessor,
};
use crate::{ShaderProgramRequest, ShaderResult, ShaderSourceProvider};

/// Borrowed inputs used to preprocess a shader program.
#[derive(Debug)]
pub struct PreprocessContext<'a, P>
where
    P: ShaderSourceProvider + ?Sized,
{
    /// Program request containing stages and combo values.
    request: &'a ShaderProgramRequest,
    /// Source provider used to resolve include directives.
    source_provider: &'a P,
}

impl<'a, P> PreprocessContext<'a, P>
where
    P: ShaderSourceProvider + ?Sized,
{
    /// Creates a preprocessing context from a request and source provider.
    #[must_use]
    pub const fn new(request: &'a ShaderProgramRequest, source_provider: &'a P) -> Self {
        Self {
            request,
            source_provider,
        }
    }

    /// Returns the shader program request.
    fn request(&self) -> &ShaderProgramRequest {
        self.request
    }

    /// Returns the source provider used for includes.
    fn source_provider(&self) -> &P {
        self.source_provider
    }

    /// Preprocesses all stage sources in the shader program request.
    ///
    /// # Errors
    ///
    /// Returns a shader error when an include cannot be resolved, an include
    /// cycle is detected, or a preprocessing directive is malformed.
    pub fn preprocess(&self) -> ShaderResult<PreprocessedProgram> {
        self.preprocess_with_mode(ConditionalMode::Evaluate)
    }

    /// Expands include directives without stripping conditional branches.
    ///
    /// This mirrors the legacy metadata scan input: include contents are
    /// visible, but annotations in inactive branches remain available to the
    /// metadata extractor.
    pub(crate) fn expand_includes_preserving_conditionals(
        &self,
    ) -> ShaderResult<PreprocessedProgram> {
        self.preprocess_with_mode(ConditionalMode::Preserve)
    }

    /// Preprocesses all stage sources using the requested conditional mode.
    fn preprocess_with_mode(
        &self,
        conditional_mode: ConditionalMode,
    ) -> ShaderResult<PreprocessedProgram> {
        let mut stages = Vec::with_capacity(self.request().stages().len());
        let macro_prelude = MacroPrelude::from(self.request().combos());

        for stage in self.request().stages() {
            let mut preprocessor = StagePreprocessor {
                stage: stage.kind(),
                source_provider: self.source_provider(),
                macros: MacroTable::from_combos(self.request().combos()),
                include_stack: Vec::new(),
                conditional_mode,
            };
            let mut source = preprocessor.preprocess_root(stage.source())?;
            macro_prelude.prepend_to(&mut source);
            stages.push(PreprocessedStage::new(stage.kind(), source));
        }

        Ok(PreprocessedProgram::new(stages.into_boxed_slice()))
    }
}
