//! Preprocessor directive line and argument parsing.

use super::SourceContext;

/// Location of a preprocessing directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectiveLocation<'a> {
    /// Source buffer containing the directive.
    pub context: SourceContext<'a>,
    /// One-based line number in the source buffer.
    pub line_number: usize,
}
