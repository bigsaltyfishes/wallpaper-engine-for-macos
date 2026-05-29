//! Naga shader compiler, reflection, and diagnostics backend.

mod compiler;
mod diagnostic;
mod reflector;

pub use compiler::NagaCompiler;
pub use reflector::NagaReflector;
