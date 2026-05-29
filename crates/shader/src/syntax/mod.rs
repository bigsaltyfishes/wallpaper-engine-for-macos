//! Lightweight syntax model for Wallpaper Engine shader legalization.

mod annotation;
mod context;
mod declaration;
mod directive;
mod function;
mod module;
mod parser;
mod source;

pub use annotation::{AnnotationKind, ShaderAnnotation};
pub use context::ParsingContext;
pub use declaration::{DeclarationKind, ShaderDeclaration, TopLevelQualifier};
pub use directive::PreprocessorDirective;
pub use function::FunctionDecl;
pub use module::{ShaderModule, SyntaxItem};
use parser::Parser;
pub use source::ShaderSourceText;
