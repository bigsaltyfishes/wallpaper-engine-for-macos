use smol_str::SmolStr;

use super::{FunctionSpecialization, ShaderResult, SourceSpan};
use crate::{ShaderDiagnostic, ShaderError, syntax::FunctionDecl};

/// Same-name functions that also need array-parameter specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionOverloads {
    /// Specialized signatures for all overloads with this name.
    pub signatures: Vec<FunctionSpecializationSignature>,
}

impl FunctionOverloads {
    /// Returns a controlled error if specialization would create duplicate
    /// same-name signatures.
    pub(super) fn ensure_unambiguous(
        &self,
        parameters: &FunctionParameterList,
        specialization: &FunctionSpecialization,
    ) -> ShaderResult<()> {
        let specialized_signature = specialization.retained_parameter_types();
        let collisions = self
            .signatures
            .iter()
            .filter(|signature| signature.retained_parameter_types() == specialized_signature)
            .count();
        let same_call_shape = self
            .signatures
            .iter()
            .filter(|signature| signature.has_array_parameters())
            .filter(|signature| signature.call_shape() == parameters.call_shape())
            .count();
        if collisions <= 1 && same_call_shape <= 1 {
            return Ok(());
        }

        Err(ShaderError::Codegen {
            diagnostics: Box::new([ShaderDiagnostic::new(
                "array-parameter specialization is ambiguous for overloaded function",
            )
            .with_pass("Codegen")]),
        })
    }
}
/// One same-name overload's signature after possible array specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionSpecializationSignature {
    /// Whether this overload itself contains fixed-array parameters.
    pub has_array_parameters: bool,
    /// Parameter type signature that would remain after specialization.
    pub retained_parameter_types: Vec<Option<SmolStr>>,
    /// Coarse syntactic argument shape accepted by the overload.
    pub call_shape: Vec<ParameterCallShape>,
}

impl FunctionSpecializationSignature {
    /// Returns whether this overload itself contains fixed-array parameters.
    pub(super) const fn has_array_parameters(&self) -> bool {
        self.has_array_parameters
    }

    /// Returns the parameter type signature that remains after specialization.
    pub(super) fn retained_parameter_types(&self) -> &[Option<SmolStr>] {
        &self.retained_parameter_types
    }

    /// Returns the coarse call-site shape.
    pub(super) fn call_shape(&self) -> &[ParameterCallShape] {
        &self.call_shape
    }
}
/// Parsed semantic parameters for one function declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionParameterList {
    /// Full source span between the function call parentheses.
    pub span: SourceSpan,
    /// Parameter declarations in source order.
    pub items: Vec<FunctionParameter>,
}

impl FunctionParameterList {
    /// Parses one function declaration's parameter list.
    pub(super) fn parse(function: &FunctionDecl<'_>) -> Self {
        let span = function.parameters_span();
        let items = function
            .parameters()
            .iter()
            .map(|parameter| FunctionParameter {
                span: parameter.span(),
                ty: Some(parameter.ty().clone()),
                array: parameter
                    .array_name()
                    .map(|name| ArrayFunctionParameter { name: name.clone() }),
            })
            .collect();

        Self { span, items }
    }

    /// Returns whether any parameter is a fixed-size array.
    pub(super) fn has_array_parameters(&self) -> bool {
        self.items.iter().any(|parameter| parameter.array.is_some())
    }

    /// Returns the parameter type signature left after specialization.
    pub(super) fn retained_parameter_types(&self) -> Vec<Option<SmolStr>> {
        self.items
            .iter()
            .filter(|parameter| parameter.array.is_none())
            .map(|parameter| parameter.ty.clone())
            .collect()
    }

    /// Returns the syntactic signature shape available at call sites.
    pub(super) fn call_shape(&self) -> Vec<ParameterCallShape> {
        self.items
            .iter()
            .map(|parameter| {
                if parameter.array.is_some() {
                    ParameterCallShape::TopLevelArrayIdentifier
                } else {
                    ParameterCallShape::AnyExpression
                }
            })
            .collect()
    }
}
/// Coarse call-site shape used when expression typing is unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParameterCallShape {
    /// A retained scalar/vector/etc expression.
    AnyExpression,
    /// A removed fixed-array parameter that must be a top-level array name.
    TopLevelArrayIdentifier,
}
/// One comma-delimited function parameter segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionParameter {
    /// Original source span for this parameter segment.
    pub span: SourceSpan,
    /// Parameter type spelling, excluding storage/precision qualifiers.
    pub ty: Option<SmolStr>,
    /// Fixed-array details, when this segment is a fixed-array parameter.
    pub array: Option<ArrayFunctionParameter>,
}
/// One fixed-array function parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ArrayFunctionParameter {
    /// Parameter identifier used inside the function body.
    pub name: SmolStr,
}

#[cfg(test)]
mod tests {
    use super::{
        FunctionOverloads, FunctionParameterList, FunctionSpecializationSignature,
        ParameterCallShape,
    };
    use crate::{
        ShaderStageKind,
        syntax::{ShaderModule, SyntaxItem},
    };

    #[test]
    fn collect_for_function_reads_same_name_overload_signatures() {
        let source = r"
            void helper(float values[4], float factor) {}
            void helper(float value) {}
            void other(float values[4]) {}
        ";
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
        let function = module
            .items()
            .iter()
            .find_map(|item| match item {
                SyntaxItem::Function(function) if function.name() == "helper" => Some(function),
                _ => None,
            })
            .expect("helper function exists");

        let mut signatures = Vec::new();
        for overload in module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(overload) if overload.name() == function.name() => Some(overload),
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

        assert_eq!(
            overloads.signatures,
            vec![
                FunctionSpecializationSignature {
                    has_array_parameters: true,
                    retained_parameter_types: vec![Some("float".into())],
                    call_shape: vec![
                        ParameterCallShape::TopLevelArrayIdentifier,
                        ParameterCallShape::AnyExpression,
                    ],
                },
                FunctionSpecializationSignature {
                    has_array_parameters: false,
                    retained_parameter_types: vec![Some("float".into())],
                    call_shape: vec![ParameterCallShape::AnyExpression],
                },
            ]
        );
    }
}
