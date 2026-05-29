//! Stage-local shader source preprocessing.

use std::fmt::Write as _;

use super::{
    ConditionalError, ConditionalExpression, ConditionalMode, ConditionalStack, DefineDirective,
    DirectiveHandlingContext, DirectiveLine, DirectiveLocation, IncludeDirective, MacroName,
    MacroTable,
};
use crate::{
    IncludePath, ShaderDiagnostic, ShaderError, ShaderResult, ShaderSourceProvider,
    ShaderStageKind, SourceSpan, syntax::PreprocessorDirective,
};

/// Preprocessed shader source for one stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreprocessedStage {
    /// Shader stage this source belongs to.
    kind: ShaderStageKind,
    /// Source after include and conditional preprocessing.
    source: String,
}

impl PreprocessedStage {
    /// Creates preprocessed stage source.
    #[must_use]
    pub fn new(kind: ShaderStageKind, source: String) -> Self {
        Self { kind, source }
    }

    /// Returns the shader stage kind.
    #[must_use]
    pub const fn kind(&self) -> ShaderStageKind {
        self.kind
    }

    /// Returns the preprocessed shader source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Stateful preprocessor for a single shader stage.
pub(super) struct StagePreprocessor<'a, P>
where
    P: ShaderSourceProvider + ?Sized,
{
    /// Stage currently being preprocessed.
    pub(super) stage: ShaderStageKind,
    /// Source provider for resolving includes.
    pub(super) source_provider: &'a P,
    /// Macro values visible to conditionals.
    pub(super) macros: MacroTable,
    /// Include stack used to reject recursive includes.
    pub(super) include_stack: Vec<IncludePath>,
    /// Conditional handling behavior for this preprocessing pass.
    pub(super) conditional_mode: ConditionalMode,
}

impl<P> StagePreprocessor<'_, P>
where
    P: ShaderSourceProvider + ?Sized,
{
    /// Preprocesses the root stage source.
    pub(super) fn preprocess_root(&mut self, source: &str) -> ShaderResult<String> {
        self.preprocess_source(source, SourceContext::Root)
    }

    /// Resolves and preprocesses an include source.
    fn preprocess_include(
        &mut self,
        path: &IncludePath,
        context: SourceContext<'_>,
        line_number: usize,
    ) -> ShaderResult<String> {
        if self.include_stack.contains(path) {
            let include_chain = self
                .include_stack
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(self.parse_error_at(
                context,
                line_number,
                format!("include cycle detected: {include_chain} -> {path}"),
            ));
        }

        self.include_stack.push(path.clone());
        let source = self.source_provider.read_to_string(path)?;
        let result = self.preprocess_source(&source, SourceContext::Include(path));
        let _removed = self.include_stack.pop();
        result
    }

    /// Preprocesses one source buffer in a root or include context.
    fn preprocess_source(
        &mut self,
        source: &str,
        context: SourceContext<'_>,
    ) -> ShaderResult<String> {
        let mut output = String::with_capacity(source.len());
        let mut conditionals = ConditionalStack::new();
        for (line_index, line) in source.lines().enumerate() {
            let line_number = line_index + 1;
            let trimmed = line.trim_start();

            if trimmed.starts_with('#') {
                let mut handling = DirectiveHandlingContext {
                    raw_line: line,
                    output: &mut output,
                    conditionals: &mut conditionals,
                    location: DirectiveLocation {
                        context,
                        line_number,
                    },
                };
                let syntax_directive =
                    PreprocessorDirective::from_token_text(trimmed, SourceSpan::default());
                self.handle_directive(DirectiveLine::from(syntax_directive), &mut handling)?;
                continue;
            }

            if conditionals.is_active() || self.conditional_mode == ConditionalMode::Preserve {
                writeln!(output, "{line}").map_err(|error| {
                    self.parse_error(format!("failed to write preprocessed source: {error}"))
                })?;
            }
        }

        if !conditionals.is_empty() {
            let Some(opening) = conditionals.innermost_opening() else {
                return Err(self.parse_error_at(
                    context,
                    source.lines().count(),
                    "unterminated conditional directive",
                ));
            };
            return Err(self.parse_error_at(
                opening.context,
                opening.line_number,
                "unterminated conditional directive",
            ));
        }

        Ok(output)
    }

    /// Applies a single preprocessor directive line.
    fn handle_directive(
        &mut self,
        directive: DirectiveLine<'_>,
        handling: &mut DirectiveHandlingContext<'_, '_>,
    ) -> ShaderResult<()> {
        let location = handling.location;

        match directive {
            DirectiveLine::Include(include) => {
                if handling.conditionals.is_active() {
                    let path = IncludeDirective::try_from(include).map_err(|message| {
                        self.parse_error_at(location.context, location.line_number, message)
                    })?;
                    let include_source = self.preprocess_include(
                        path.path(),
                        location.context,
                        location.line_number,
                    )?;
                    handling.output.push_str(&include_source);
                } else if self.conditional_mode == ConditionalMode::Preserve {
                    writeln!(handling.output, "{}", handling.raw_line).map_err(|error| {
                        self.parse_error(format!("failed to write preprocessed source: {error}"))
                    })?;
                }
                Ok(())
            }
            DirectiveLine::Define(define) => {
                if handling.conditionals.is_active() {
                    let define = DefineDirective::try_from(define).map_err(|message| {
                        self.parse_error_at(location.context, location.line_number, message)
                    })?;
                    self.macros.define(define.name().as_str(), define.value());
                    writeln!(handling.output, "#{}", directive.raw()).map_err(|error| {
                        self.parse_error(format!("failed to write preprocessed source: {error}"))
                    })?;
                }
                Ok(())
            }
            DirectiveLine::Ifdef(conditional) => self.handle_macro_condition(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
                false,
            ),
            DirectiveLine::Ifndef(conditional) => self.handle_macro_condition(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
                true,
            ),
            DirectiveLine::If(conditional) => self.handle_if_condition(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
            ),
            DirectiveLine::Elif(conditional) => self.handle_elif_condition(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
            ),
            DirectiveLine::Else(conditional) => self.handle_conditional_boundary(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
                "else",
            ),
            DirectiveLine::Endif(conditional) => self.handle_conditional_boundary(
                conditional,
                handling.conditionals,
                location.context,
                location.line_number,
                "endif",
            ),
            DirectiveLine::Require(_require) => Ok(()),
            DirectiveLine::Other(other) => {
                if handling.conditionals.is_active() {
                    writeln!(handling.output, "#{}", other.raw_text()).map_err(|error| {
                        self.parse_error(format!("failed to write preprocessed source: {error}"))
                    })?;
                }
                Ok(())
            }
        }?;

        if self.conditional_mode == ConditionalMode::Preserve
            && !matches!(
                directive,
                DirectiveLine::Include(_) | DirectiveLine::Define(_) | DirectiveLine::Require(_)
            )
        {
            writeln!(handling.output, "{}", handling.raw_line).map_err(|error| {
                self.parse_error(format!("failed to write preprocessed source: {error}"))
            })?;
        }

        Ok(())
    }

    /// Pushes an `#ifdef` or `#ifndef` frame.
    fn handle_macro_condition<'src>(
        &self,
        conditional: crate::syntax::PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'src>,
        context: SourceContext<'src>,
        line_number: usize,
        negate: bool,
    ) -> ShaderResult<()> {
        let condition_active = if conditionals.is_active() {
            let macro_name = MacroName::try_from(conditional.body_text())
                .map_err(|message| self.parse_error_at(context, line_number, message))?;
            self.macros.contains(macro_name.as_str()) ^ negate
        } else {
            false
        };
        conditionals.push(
            condition_active,
            DirectiveLocation {
                context,
                line_number,
            },
        );
        Ok(())
    }

    /// Pushes an `#if` expression frame.
    fn handle_if_condition<'src>(
        &self,
        conditional: crate::syntax::PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'src>,
        context: SourceContext<'src>,
        line_number: usize,
    ) -> ShaderResult<()> {
        let is_active = if conditionals.is_active() {
            ConditionalExpression::try_from(conditional.body_text())
                .and_then(|expression| expression.evaluate(&self.macros))
                .map_err(|message| self.parse_error_at(context, line_number, message))?
        } else {
            false
        };
        conditionals.push(
            is_active,
            DirectiveLocation {
                context,
                line_number,
            },
        );
        Ok(())
    }

    /// Enters an `#elif` expression arm.
    fn handle_elif_condition<'src>(
        &self,
        conditional: crate::syntax::PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'src>,
        context: SourceContext<'src>,
        line_number: usize,
    ) -> ShaderResult<()> {
        let should_evaluate = conditionals
            .should_evaluate_elif()
            .map_err(|error| self.conditional_error(context, line_number, error))?;
        let is_active = if should_evaluate {
            ConditionalExpression::try_from(conditional.body_text())
                .and_then(|expression| expression.evaluate(&self.macros))
                .map_err(|message| self.parse_error_at(context, line_number, message))?
        } else {
            false
        };
        conditionals
            .enter_elif(is_active)
            .map_err(|error| self.conditional_error(context, line_number, error))
    }

    /// Handles an `#else` or `#endif` stack transition.
    fn handle_conditional_boundary(
        &self,
        conditional: crate::syntax::PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'_>,
        context: SourceContext<'_>,
        line_number: usize,
        directive_name: &str,
    ) -> ShaderResult<()> {
        if !conditional.body_text().is_empty() {
            return Err(self.parse_error_at(
                context,
                line_number,
                format!("#{directive_name} does not accept trailing tokens"),
            ));
        }

        let result = match directive_name {
            "else" => conditionals.enter_else(),
            "endif" => match conditionals.pop() {
                Err(ConditionalError::UnmatchedEndif) => Ok(()),
                Err(error) => Err(error),
                result => result,
            },
            _ => Ok(()),
        };
        result.map_err(|error| self.conditional_error(context, line_number, error))
    }

    /// Converts a conditional stack error into a stage-scoped diagnostic.
    fn conditional_error(
        &self,
        context: SourceContext<'_>,
        line_number: usize,
        error: ConditionalError,
    ) -> ShaderError {
        let message = match error {
            ConditionalError::UnmatchedElif => "unmatched #elif directive",
            ConditionalError::ElifAfterElse => "#elif after #else directive",
            ConditionalError::UnmatchedElse => "unmatched #else directive",
            ConditionalError::DuplicateElse => "duplicate #else directive",
            ConditionalError::UnmatchedEndif => "unmatched #endif directive",
        };
        self.parse_error_at(context, line_number, message)
    }

    /// Builds a stage-scoped parse error.
    fn parse_error(&self, message: impl Into<String>) -> ShaderError {
        ShaderError::Parse {
            diagnostics: Box::new([ShaderDiagnostic::new(message).with_stage(self.stage)]),
        }
    }

    /// Builds a stage-scoped parse error with source location text.
    fn parse_error_at(
        &self,
        context: SourceContext<'_>,
        line_number: usize,
        message: impl AsRef<str>,
    ) -> ShaderError {
        let mut contextual_message = String::new();
        match context {
            SourceContext::Root => {
                let _ = write!(
                    contextual_message,
                    "stage {:?} line {}: {}",
                    self.stage,
                    line_number,
                    message.as_ref()
                );
            }
            SourceContext::Include(path) => {
                let _ = write!(
                    contextual_message,
                    "include {} line {}: {}",
                    path,
                    line_number,
                    message.as_ref()
                );
            }
        }

        self.parse_error(contextual_message)
    }
}

/// Identifies whether diagnostics refer to root source or an include.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SourceContext<'a> {
    /// Root shader stage source.
    Root,
    /// Source loaded through an include path.
    Include(&'a IncludePath),
}
