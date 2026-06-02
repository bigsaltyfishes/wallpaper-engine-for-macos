//! Stage-local shader source preprocessing.

use std::fmt::Write as _;

use super::{
    ConditionalError, ConditionalExpression, ConditionalMode, ConditionalStack, DirectiveLocation,
    MacroName, MacroTable,
};
use crate::{
    IncludePath, ShaderDiagnostic, ShaderError, ShaderResult, ShaderSourceProvider,
    ShaderStageKind, SourceSpan, syntax::PreprocessorDirective,
};

/// Diagnostic for an include directive without a quoted path.
const INCLUDE_PATH_ERROR: &str = "#include expects a quoted include path";
/// Diagnostic for a define directive without a macro name.
const DEFINE_NAME_ERROR: &str = "#define expects a macro name";
/// Diagnostic for a define directive with an invalid macro name.
const DEFINE_INVALID_NAME_ERROR: &str = "#define macro name is invalid";

/// Preprocessed shader source for one stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreprocessedStage {
    /// Shader stage this source belongs to.
    kind: ShaderStageKind,
    /// Source after include and conditional preprocessing.
    source: String,
    /// Contiguous leading object-like macro values visible to syntax facts.
    macros: MacroTable,
}

impl PreprocessedStage {
    /// Creates preprocessed stage source.
    #[must_use]
    pub fn new(kind: ShaderStageKind, source: String) -> Self {
        let mut macros = MacroTable::new();
        for line in source.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                break;
            }
            let directive = PreprocessorDirective::from_token_text(trimmed, SourceSpan::default());
            if !directive.is_define() {
                break;
            }
            let Ok(Some(parts)) = directive.define_parts() else {
                continue;
            };
            let name = parts.name_text();
            if parts.has_explicit_value() && MacroName::parse(name).is_ok() {
                macros.define(name, parts.value().as_str());
            }
        }
        Self {
            kind,
            source,
            macros,
        }
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

    /// Returns contiguous leading object-like macro values.
    #[must_use]
    pub fn macros(&self) -> &MacroTable {
        &self.macros
    }
}

/// Stateful preprocessor for a single shader stage.
pub(super) struct StagePreprocessor<'a, P>
where
    P: ShaderSourceProvider + ?Sized,
{
    /// Stage currently being preprocessed.
    pub stage: ShaderStageKind,
    /// Source provider for resolving includes.
    pub source_provider: &'a P,
    /// Macro values visible to conditionals.
    pub macros: MacroTable,
    /// Include stack used to reject recursive includes.
    pub include_stack: Vec<IncludePath>,
    /// Conditional handling behavior for this preprocessing pass.
    pub conditional_mode: ConditionalMode,
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
                let directive =
                    PreprocessorDirective::from_token_text(trimmed, SourceSpan::default());
                self.handle_directive(
                    directive,
                    line,
                    &mut output,
                    &mut conditionals,
                    DirectiveLocation {
                        context,
                        line_number,
                    },
                )?;
                continue;
            }

            if conditionals.is_active() || self.conditional_mode == ConditionalMode::Preserve {
                let line = line.trim_end().strip_suffix('\\').unwrap_or(line);
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
    fn handle_directive<'context>(
        &mut self,
        directive: PreprocessorDirective<'_>,
        raw_line: &str,
        output: &mut String,
        conditionals: &mut ConditionalStack<'context>,
        location: DirectiveLocation<'context>,
    ) -> ShaderResult<()> {
        let context = location.context;
        let line_number = location.line_number;
        let preserve_directive = self.conditional_mode == ConditionalMode::Preserve
            && !(directive.is_include() || directive.is_define() || directive.is_require());

        if directive.is_include() {
            if conditionals.is_active() {
                let path = directive
                    .include_path()
                    .map_err(|message| self.parse_error_at(context, line_number, message))?
                    .ok_or_else(|| self.parse_error_at(context, line_number, INCLUDE_PATH_ERROR))?;
                let include_source = self.preprocess_include(&path, context, line_number)?;
                output.push_str(&include_source);
            } else if self.conditional_mode == ConditionalMode::Preserve {
                writeln!(output, "{raw_line}").map_err(|error| {
                    self.parse_error(format!("failed to write preprocessed source: {error}"))
                })?;
            }
        } else if directive.is_define() {
            if conditionals.is_active() {
                let parts = directive
                    .define_parts()
                    .map_err(|message| self.parse_error_at(context, line_number, message))?
                    .ok_or_else(|| self.parse_error_at(context, line_number, DEFINE_NAME_ERROR))?;
                let name = MacroName::parse(parts.name_text()).map_err(|_error| {
                    self.parse_error_at(context, line_number, DEFINE_INVALID_NAME_ERROR)
                })?;
                self.macros.define(name.as_str(), parts.value().as_str());
                writeln!(output, "#{}", directive.raw_text()).map_err(|error| {
                    self.parse_error(format!("failed to write preprocessed source: {error}"))
                })?;
            }
        } else if let Some(conditional) = directive.conditional() {
            if conditional.is_ifdef() {
                self.handle_macro_condition(directive, conditionals, context, line_number, false)?;
            } else if conditional.is_ifndef() {
                self.handle_macro_condition(directive, conditionals, context, line_number, true)?;
            } else if conditional.is_if() {
                self.handle_if_condition(directive, conditionals, context, line_number)?;
            } else if conditional.is_elif() {
                self.handle_elif_condition(directive, conditionals, context, line_number)?;
            } else {
                self.handle_conditional_boundary(directive, conditionals, context, line_number)?;
            }
        } else if !directive.is_require() && conditionals.is_active() {
            writeln!(output, "#{}", directive.raw_text()).map_err(|error| {
                self.parse_error(format!("failed to write preprocessed source: {error}"))
            })?;
        }

        if preserve_directive {
            writeln!(output, "{raw_line}").map_err(|error| {
                self.parse_error(format!("failed to write preprocessed source: {error}"))
            })?;
        }

        Ok(())
    }

    /// Pushes an `#ifdef` or `#ifndef` frame.
    fn handle_macro_condition<'context>(
        &self,
        conditional: PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'context>,
        context: SourceContext<'context>,
        line_number: usize,
        negate: bool,
    ) -> ShaderResult<()> {
        let condition_active = if conditionals.is_active() {
            let macro_name = MacroName::parse(conditional.body_text())
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
    fn handle_if_condition<'context>(
        &self,
        conditional: PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'context>,
        context: SourceContext<'context>,
        line_number: usize,
    ) -> ShaderResult<()> {
        let is_active = if conditionals.is_active() {
            ConditionalExpression::parse(conditional.body_text())
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
    fn handle_elif_condition<'context>(
        &self,
        conditional: PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'context>,
        context: SourceContext<'context>,
        line_number: usize,
    ) -> ShaderResult<()> {
        let should_evaluate = conditionals
            .should_evaluate_elif()
            .map_err(|error| self.conditional_error(context, line_number, error))?;
        let is_active = if should_evaluate {
            ConditionalExpression::parse(conditional.body_text())
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
        conditional: PreprocessorDirective<'_>,
        conditionals: &mut ConditionalStack<'_>,
        context: SourceContext<'_>,
        line_number: usize,
    ) -> ShaderResult<()> {
        if !conditional.body_text().is_empty() {
            let directive_name = conditional.name_text();
            return Err(self.parse_error_at(
                context,
                line_number,
                format!("#{directive_name} does not accept trailing tokens"),
            ));
        }

        let result = if let Some(conditional) = conditional.conditional() {
            match conditional.kind() {
                crate::syntax::ConditionalDirectiveKind::Else => conditionals.enter_else(),
                crate::syntax::ConditionalDirectiveKind::Endif => match conditionals.pop() {
                    Err(ConditionalError::UnmatchedEndif) => Ok(()),
                    Err(error) => Err(error),
                    result => result,
                },
                _ => Ok(()),
            }
        } else {
            Ok(())
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
pub enum SourceContext<'a> {
    /// Root shader stage source.
    Root,
    /// Source loaded through an include path.
    Include(&'a IncludePath),
}
