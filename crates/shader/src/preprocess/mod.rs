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
use directives::DirectiveLocation;
use macros::MacroName;
pub use macros::MacroTable;
pub use program::PreprocessedProgram;
pub use stage::PreprocessedStage;
use stage::{SourceContext, StagePreprocessor};
