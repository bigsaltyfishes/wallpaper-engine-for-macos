//! Lightweight syntax model for Wallpaper Engine shader codegen.

mod annotation;
mod call;
mod context;
mod declaration;
mod directive;
mod function;
mod interface_use;
mod module;
mod parser;
mod source;

pub use annotation::{AnnotationKind, ShaderAnnotation};
pub use call::{CallArgument, CallArguments, FunctionCall, FunctionCalls};
pub use context::ParsingContext;
pub use declaration::{
    DeclarationArraySize, DeclarationArraySuffix, DeclarationKind, DeclarationLayout,
    ShaderDeclaration, TopLevelQualifier,
};
pub use directive::{ConditionalDirectiveKind, PreprocessorDirective};
pub use function::{FunctionDecl, FunctionDeclSpans, FunctionParameter};
pub use interface_use::{InterfaceUseFacts, InterfaceUseQuery};
pub use module::{ModuleFunctions, ShaderModule, SyntaxItem};
use parser::Parser;
pub use source::ShaderSourceText;
