//! Rewrites legacy array-parameter helpers into Naga-compatible GLSL.

/// Local array alias lowering.
mod aliases;
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
    aliases::ArrayAliases,
    calls::CallArguments,
    scopes::ArrayParameterUseScanner,
    signatures::{FunctionOverloads, FunctionParameterList, FunctionSpecializationSignature},
    specialization::FunctionSpecialization,
};
use super::{
    ARRAY_PARAMETERS, CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES,
    RESERVED_IDENTIFIERS, StrategyContext,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{
        Fixup, ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
    },
    syntax::{ShaderModule, SyntaxItem},
    tokenizer::TypedToken,
};

/// Specializes fixed-array function parameters to the global arrays passed by
/// every call. Naga's GLSL frontend accepts uniform array indexing, but does
/// not register user functions that take legacy array parameters.
struct ArrayParametersStrategy;
#[distributed_slice(GENERAL_POLICIES)]
static ARRAY_PARAMETERS_POLICY: CodegenStrategy = CodegenStrategy {
    name: ARRAY_PARAMETERS,
    stage: CodegenStage::SemanticRewrite,
    after: &[RESERVED_IDENTIFIERS],
    emitter: &ArrayParametersStrategy,
};

impl Emitable for ArrayParametersStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let module = context.context().module;
        let mut aliases = ArrayAliases::default();
        aliases.collect(module);
        let mut alias_fixups = Vec::new();
        for alias in aliases.items {
            alias.emit(module, &mut alias_fixups);
        }
        for fixup in alias_fixups {
            context.context().fixups.push(fixup);
        }

        for function in module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        }) {
            let mut signatures = Vec::new();
            for overload in module.items().iter().filter_map(|item| match item {
                SyntaxItem::Function(overload) if overload.name() == function.name() => {
                    Some(overload)
                }
                _ => None,
            }) {
                let parameters = FunctionParameterList::parse(overload);
                signatures.push(FunctionSpecializationSignature {
                    has_array_parameters: parameters.has_array_parameters(),
                    retained_parameter_types: parameters.retained_parameter_types(),
                    call_shape: parameters.call_shape(),
                });
            }
            let overloads = FunctionOverloads { signatures };
            let parameters = FunctionParameterList::parse(function);
            if !parameters.has_array_parameters() {
                continue;
            }
            let arguments = CallArguments::new(module, function.name(), &parameters)?;
            if arguments.calls.is_empty() {
                continue;
            }
            let specialization = FunctionSpecialization::new(module, &parameters, &arguments)?;
            overloads.ensure_unambiguous(&parameters, &specialization)?;

            context
                .context()
                .fixups
                .push(Fixup::replace(parameters.span, specialization.parameters));
            for (parameter, argument) in specialization.array_parameters {
                let Some(body) = module
                    .token_stream()
                    .cursor()
                    .contained_byte_range(function.body_span().start(), function.body_span().end())
                else {
                    continue;
                };
                for span in (ArrayParameterUseScanner { body }).use_spans(
                    module,
                    module.token_stream().cursor(),
                    parameter.name.as_str(),
                ) {
                    context
                        .context()
                        .fixups
                        .push(Fixup::replace(span, argument.to_string()));
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
