//! Preprocessor directive line and argument parsing.

use super::{ConditionalStack, SourceContext};
use crate::{IncludePath, syntax::PreprocessorDirective};

/// Location of a preprocessing directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DirectiveLocation<'a> {
    /// Source buffer containing the directive.
    pub(super) context: SourceContext<'a>,
    /// One-based line number in the source buffer.
    pub(super) line_number: usize,
}

/// Borrowed state needed while applying one directive line.
pub(super) struct DirectiveHandlingContext<'output, 'src> {
    /// Original source line including the leading directive marker.
    pub(super) raw_line: &'src str,
    /// Preprocessed source being built.
    pub(super) output: &'output mut String,
    /// Active conditional stack.
    pub(super) conditionals: &'output mut ConditionalStack<'src>,
    /// Directive source location.
    pub(super) location: DirectiveLocation<'src>,
}

/// Typed preprocessor directive categories used by stage preprocessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DirectiveLine<'src> {
    /// `#include`.
    Include(PreprocessorDirective<'src>),
    /// `#define`.
    Define(PreprocessorDirective<'src>),
    /// `#ifdef`.
    Ifdef(PreprocessorDirective<'src>),
    /// `#ifndef`.
    Ifndef(PreprocessorDirective<'src>),
    /// `#if`.
    If(PreprocessorDirective<'src>),
    /// `#elif`.
    Elif(PreprocessorDirective<'src>),
    /// `#else`.
    Else(PreprocessorDirective<'src>),
    /// `#endif`.
    Endif(PreprocessorDirective<'src>),
    /// Wallpaper Engine `#require`.
    Require(PreprocessorDirective<'src>),
    /// Any other directive.
    Other(PreprocessorDirective<'src>),
}

impl<'src> From<PreprocessorDirective<'src>> for DirectiveLine<'src> {
    fn from(directive: PreprocessorDirective<'src>) -> Self {
        match directive.name_text() {
            "include" => Self::Include(directive),
            "define" => Self::Define(directive),
            "ifdef" => Self::Ifdef(directive),
            "ifndef" => Self::Ifndef(directive),
            "if" => Self::If(directive),
            "elif" => Self::Elif(directive),
            "else" => Self::Else(directive),
            "endif" => Self::Endif(directive),
            "require" => Self::Require(directive),
            _ => Self::Other(directive),
        }
    }
}

impl<'src> DirectiveLine<'src> {
    /// Returns the retained syntax directive.
    pub(super) const fn directive(self) -> PreprocessorDirective<'src> {
        match self {
            Self::Include(directive)
            | Self::Define(directive)
            | Self::Ifdef(directive)
            | Self::Ifndef(directive)
            | Self::If(directive)
            | Self::Elif(directive)
            | Self::Else(directive)
            | Self::Endif(directive)
            | Self::Require(directive)
            | Self::Other(directive) => directive,
        }
    }

    /// Returns the directive text without the leading `#`.
    pub(super) fn raw(self) -> &'src str {
        self.directive().raw_text()
    }
}

/// Parsed `#include` directive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct IncludeDirective {
    /// Resolved typed include path.
    path: IncludePath,
}

impl IncludeDirective {
    /// Returns this directive's include path.
    pub(super) fn path(&self) -> &IncludePath {
        &self.path
    }
}

impl TryFrom<PreprocessorDirective<'_>> for IncludeDirective {
    type Error = &'static str;

    fn try_from(directive: PreprocessorDirective<'_>) -> Result<Self, Self::Error> {
        let Some(path) = directive.include_path()? else {
            return Err("#include expects a quoted include path");
        };
        Ok(Self { path })
    }
}
