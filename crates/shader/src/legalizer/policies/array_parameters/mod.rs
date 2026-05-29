//! Rewrites legacy array-parameter helpers into Naga-compatible GLSL.

/// Call-site argument parsing for array parameter specialization.
mod calls;
/// Scope-aware scans for specialized array parameter uses.
mod scopes;
/// Function signature parsing for array parameter specialization.
mod signatures;
/// Specialization planning for array parameter functions.
mod specialization;

use linkme::distributed_slice;

use self::{
    calls::{CallArguments, CallArgumentsSource},
    scopes::ArrayParameterUses,
    signatures::{
        ArrayFunctionParameters, ArrayFunctionParametersSource, FunctionOverloadSource,
        FunctionOverloads, ParameterEnd, SingleIdentifier,
    },
    specialization::{
        FunctionSpecialization, SegmentSpan, SpecializationSource, TopLevelArrayDeclaration,
    },
};
use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, SourceSpan,
    legalizer::{
        Fixup, FunctionCallIndex, FunctionParameterQualifier, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, TokenSearch,
        tokens::BalancedTokens,
    },
    lexer::{Token, TokenKind},
    syntax::{ShaderModule, SyntaxItem},
};

/// Specializes fixed-array function parameters to the global arrays passed by
/// every call. Naga's GLSL frontend accepts uniform array indexing, but does
/// not register user functions that take legacy array parameters.
struct ArrayParametersPolicy;
#[distributed_slice(GENERAL_POLICIES)]
static ARRAY_PARAMETERS_POLICY: &dyn Emitable = &ArrayParametersPolicy;
impl Emitable for ArrayParametersPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let module = context.context().module;
        for function in module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        }) {
            let overloads =
                FunctionOverloads::try_from(FunctionOverloadSource { module, function })?;
            let Some(parameters) =
                Option::<ArrayFunctionParameters<'_>>::try_from(ArrayFunctionParametersSource {
                    tokens: module.tokens(),
                    function,
                })?
            else {
                continue;
            };
            let Some(arguments) = Option::<CallArguments<'_>>::try_from(CallArgumentsSource {
                module,
                name: function.name(),
                parameters: &parameters,
            })?
            else {
                continue;
            };
            let Some(specialization) =
                Option::<FunctionSpecialization<'_>>::try_from(SpecializationSource {
                    module,
                    parameters: &parameters,
                    arguments: &arguments,
                })?
            else {
                return Err(ShaderError::Legalize {
                    diagnostics: Box::new([ShaderDiagnostic::new(
                        "array-parameter specialization requires each array parameter to use one \
                         stable top-level array argument",
                    )
                    .with_pass("Legalizer")]),
                });
            };
            overloads.ensure_unambiguous(&parameters, &specialization)?;

            context
                .context()
                .fixups
                .push(Fixup::replace(parameters.span, specialization.parameters));
            for (parameter, argument) in specialization.array_parameters {
                for span in (ArrayParameterUses {
                    tokens: module.tokens(),
                    name: parameter.name,
                    start: function.body_span().start(),
                    end: function.body_span().end(),
                })
                .spans()
                {
                    context
                        .context()
                        .fixups
                        .push(Fixup::replace(span, argument));
                }
            }
            for call in specialization.calls {
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.span, call.arguments));
            }
        }

        Ok(())
    }
}
