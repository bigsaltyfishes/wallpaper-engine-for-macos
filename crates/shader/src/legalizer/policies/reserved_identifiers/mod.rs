//! Reserved identifier legalization.

/// Function-name collisions with reserved identifiers.
mod functions;
/// Local identifier collisions with reserved identifiers.
mod locals;
/// User-defined `mod` compatibility classification.
mod mod_function;
/// Scalar facts used for reserved function compatibility.
mod scalar;

use std::collections::BTreeSet;

use linkme::distributed_slice;

use self::{
    functions::{StageInterfaceNames, UserFunctionCollision},
    locals::LocalCollisionSet,
    mod_function::{ClassifiedModCollision, ModArgument, ModCall, ModCollisionClass, TokenRange},
    scalar::{ScalarExpression, ScalarTypeFacts},
};
use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{
        DeclarationDeclarators, Fixup, FunctionCall, FunctionCallIndex, FunctionParameterQualifier,
        LocalDeclaration, LocalDeclarationStart, LocalTypeName, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
    },
    lexer::{Token, TokenKind},
    syntax::{FunctionDecl, ShaderModule, SyntaxItem},
};

/// Renames user-defined functions that collide with GLSL builtins.
struct ReservedIdentifiersPolicy;
#[distributed_slice(GENERAL_POLICIES)]
static RESERVED_IDENTIFIERS_POLICY: &dyn Emitable = &ReservedIdentifiersPolicy;
impl Emitable for ReservedIdentifiersPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        for collision in LocalCollisionSet::from(context.context()) {
            collision.emit(context.context());
        }
        UserFunctionCollision {
            source: "mod",
            replacement: "_we_user_mod",
        }
        .emit(context);
        UserFunctionCollision {
            source: "sample",
            replacement: "_we_user_sample",
        }
        .emit(context);
        Ok(())
    }
}
