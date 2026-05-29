//! Shader source preprocessing.

mod conditionals;
mod context;
mod directives;
mod macros;
mod program;
mod stage;

pub use conditionals::ConditionalStack;
use conditionals::{ConditionalError, ConditionalExpression, ConditionalMode};
pub use context::PreprocessContext;
use directives::{DirectiveHandlingContext, DirectiveLine, DirectiveLocation, IncludeDirective};
pub use macros::MacroTable;
use macros::{DefineDirective, MacroName, MacroPrelude};
pub use program::PreprocessedProgram;
pub use stage::PreprocessedStage;
use stage::{SourceContext, StagePreprocessor};
