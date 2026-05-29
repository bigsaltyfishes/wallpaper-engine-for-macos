use smol_str::SmolStr;

use super::{
    ShaderModule, SyntaxItem,
    calls::{CallArguments, SpecializedCall},
    signatures::{ArrayFunctionParameter, FunctionParameterList},
};
use crate::{ShaderDiagnostic, ShaderError};

/// Safe specialization plan for one function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionSpecialization {
    /// Replacement function parameter list.
    pub parameters: String,
    /// Parameter type signature left after specialization.
    pub retained_parameter_types: Vec<Option<SmolStr>>,
    /// Fixed-array parameters paired with stable global array arguments.
    pub array_parameters: Vec<(ArrayFunctionParameter, SmolStr)>,
    /// Replacement argument lists for call sites.
    pub calls: Vec<SpecializedCall>,
}

impl FunctionSpecialization {
    /// Constructs a safe specialization plan for one array-parameter function.
    #[inline]
    #[allow(clippy::single_call_fn)]
    pub(super) fn new(
        module: &ShaderModule<'_>,
        parameters: &FunctionParameterList,
        arguments: &CallArguments,
    ) -> Result<Self, ShaderError> {
        let calls = &arguments.calls;
        let mut array_parameters = Vec::new();
        for (index, parameter) in parameters.items.iter().enumerate() {
            let Some(array_parameter) = parameter.array.clone() else {
                continue;
            };
            let mut stable = None;
            for call in calls {
                let Some(identifier) = call.items.get(index).and_then(|argument| {
                    let identifier = argument.identifier.as_ref()?;
                    module
                        .has_top_level_array_declaration(identifier.as_str())
                        .then_some(identifier.clone())
                }) else {
                    stable = None;
                    break;
                };
                if stable
                    .as_ref()
                    .is_some_and(|existing| existing != &identifier)
                {
                    stable = None;
                    break;
                }
                stable = Some(identifier);
            }
            let Some(identifier) = stable else {
                array_parameters.clear();
                break;
            };
            array_parameters.push((array_parameter, identifier));
        }

        if array_parameters.is_empty() {
            return Err(ShaderError::Codegen {
                diagnostics: Box::new([ShaderDiagnostic::new(
                    "array-parameter specialization requires each array parameter to use one \
                     stable top-level array argument",
                )
                .with_pass("Codegen")]),
            });
        }

        Ok(Self {
            parameters: parameters
                .items
                .iter()
                .filter(|parameter| parameter.array.is_none())
                .map(|parameter| module.slice(parameter.span))
                .collect::<Vec<_>>()
                .join(", "),
            retained_parameter_types: parameters.retained_parameter_types(),
            array_parameters,
            calls: calls
                .iter()
                .map(|call| {
                    let retained_arguments = call
                        .items
                        .iter()
                        .zip(parameters.items.iter())
                        .filter(|(_argument, parameter)| parameter.array.is_none())
                        .map(|(argument, _parameter)| module.slice(argument.span))
                        .collect::<Vec<_>>();
                    SpecializedCall {
                        span: call.span,
                        arguments: retained_arguments.join(", "),
                    }
                })
                .collect(),
        })
    }

    /// Returns the parameter type signature left after specialization.
    pub(super) fn retained_parameter_types(&self) -> &[Option<SmolStr>] {
        &self.retained_parameter_types
    }
}

impl ShaderModule<'_> {
    /// Returns whether the module contains this top-level fixed-array
    /// declaration.
    #[must_use]
    pub fn has_top_level_array_declaration(&self, name: &str) -> bool {
        self.items().iter().any(|item| {
            let SyntaxItem::Declaration(declaration) = item else {
                return false;
            };
            declaration.name() == Some(name) && declaration.array_suffix().is_some()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{ShaderStageKind, syntax::ShaderModule};

    #[test]
    fn has_top_level_array_declaration_finds_fixed_array() {
        let source = "uniform float values[4];\nvoid main() {}\n";
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");

        assert!(module.has_top_level_array_declaration("values"));
        assert!(!module.has_top_level_array_declaration("missing"));
    }
}
