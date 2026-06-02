use smol_str::SmolStr;

use crate::{
    codegen::{
        ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
        expressions::analysis::{
            Lvalue, ScalarExpressionAnalyzer, ScalarExpressionFacts, ScalarExpressionFlavor,
            ScalarType,
        },
    },
    syntax::{ShaderModule, SyntaxItem},
    tokenizer::{TokenCursor, TypedToken, TypedTokenFacts},
};

/// Known scalar symbol facts from simple declarations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SymbolFacts<'src> {
    /// Scoped declarations in source order.
    pub bindings: Vec<SymbolBinding>,
    /// Object-like numeric macro definitions visible across the source.
    pub macros: Vec<MacroSymbol<'src>>,
}

impl<'src> SymbolFacts<'src> {
    /// Constructs scalar symbol facts from a parsed shader module.
    #[inline]
    #[allow(clippy::single_call_fn)]
    pub(super) fn new(module: &ShaderModule<'src>) -> Self {
        let scoped_facts = ScopedDeclarationFacts::collect(
            module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );
        Self {
            bindings: scoped_facts
                .declarations()
                .iter()
                .map(|declaration| SymbolBinding {
                    name: declaration.name().into(),
                    ty: match declaration.ty() {
                        "bool" => ScalarType::Bool,
                        "int" => ScalarType::Int,
                        "uint" => ScalarType::Uint,
                        "float" | "float1" => ScalarType::Float,
                        "vec2" | "vec3" | "vec4" | "float2" | "float3" | "float4" => {
                            ScalarType::FloatVector
                        }
                        _ => ScalarType::NonFloatAggregate,
                    },
                    visible_start: declaration.visible_start(),
                    scope_end: declaration.scope_end(),
                })
                .collect(),
            macros: module
                .items()
                .iter()
                .filter_map(|item| match item {
                    SyntaxItem::Directive(directive) => {
                        let parts = directive.define_parts().ok().flatten()?;
                        let name = parts.object_like_name_text()?;
                        let value = parts.simple_replacement_text()?;
                        let ty = ScalarType::classify_numeric_literal(value)?;
                        Some(MacroSymbol { name, ty })
                    }
                    SyntaxItem::Declaration(_)
                    | SyntaxItem::Function(_)
                    | SyntaxItem::Annotation(_)
                    | SyntaxItem::Opaque(_) => None,
                })
                .collect(),
        }
    }

    /// Returns whether `kind` clearly denotes a boolean expression.
    pub(super) fn bool_identifier(&self, tokens: TokenCursor<'_>, index: usize) -> bool {
        let TypedToken::Identifier(name) = tokens[index].kind() else {
            return false;
        };

        matches!(name.as_str(), "true" | "false")
            || matches!(self.visible_type(name, index), Some(ScalarType::Bool))
    }

    /// Returns whether `lvalue` is known to be a float scalar/component.
    pub(super) fn float_lvalue(&self, lvalue: &Lvalue) -> bool {
        match self.visible_type(lvalue.base.as_str(), lvalue.end) {
            Some(ScalarType::Float) => !lvalue.has_member,
            Some(ScalarType::FloatVector) => lvalue.has_member,
            _ => false,
        }
    }

    /// Returns the known scalar numeric type for a condition expression.
    pub(super) fn numeric_expression_type(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        start: usize,
        end: usize,
    ) -> Option<NumericScalarType> {
        match (ScalarExpressionAnalyzer {
            facts: self,
            token_facts,
            flavor: ScalarExpressionFlavor::NumericCondition,
        })
        .range_type(tokens, start, end)?
        {
            ScalarType::Int => Some(NumericScalarType::Int),
            ScalarType::Uint => Some(NumericScalarType::Uint),
            ScalarType::Float => Some(NumericScalarType::Float),
            ScalarType::Bool | ScalarType::FloatVector | ScalarType::NonFloatAggregate => None,
        }
    }

    /// Returns the nearest visible declaration type for `name` at `index`.
    pub(super) fn visible_type(&self, name: &str, index: usize) -> Option<ScalarType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(index))
            .map(|binding| binding.ty)
            .or_else(|| {
                self.macros
                    .iter()
                    .rev()
                    .find(|symbol| symbol.name == name)
                    .map(|symbol| symbol.ty)
            })
    }
}

impl ScalarExpressionFacts for SymbolFacts<'_> {
    fn visible_type(&self, name: &str, index: usize) -> Option<ScalarType> {
        self.visible_type(name, index)
    }

    fn float_lvalue(&self, lvalue: &Lvalue) -> bool {
        self.float_lvalue(lvalue)
    }
}

/// Numeric object-like macro usable in expression type inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacroSymbol<'src> {
    /// Macro identifier.
    pub name: &'src str,
    /// Numeric scalar type of the replacement.
    pub ty: ScalarType,
}

/// One scoped symbol binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SymbolBinding {
    /// Declared name.
    pub name: SmolStr,
    /// Declared type class.
    pub ty: ScalarType,
    /// First token where this binding may be referenced.
    pub visible_start: usize,
    /// First token outside this binding's scope.
    pub scope_end: usize,
}

impl SymbolBinding {
    /// Returns whether this binding is visible at `index`.
    const fn visible_at(&self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}

/// Type facts needed by control-flow coercions.
pub(super) type SymbolType = ScalarType;

/// Scalar numeric type facts for condition coercion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NumericScalarType {
    /// Signed integer scalar.
    Int,
    /// Unsigned integer scalar.
    Uint,
    /// Floating-point scalar.
    Float,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_type_classifies_numeric_literals() {
        assert_eq!(
            SymbolType::classify_numeric_literal("1"),
            Some(SymbolType::Int)
        );
        assert_eq!(
            SymbolType::classify_numeric_literal("1u"),
            Some(SymbolType::Uint)
        );
        assert_eq!(
            SymbolType::classify_numeric_literal("1.0"),
            Some(SymbolType::Float)
        );
        assert_eq!(SymbolType::classify_numeric_literal("@"), None);
    }
}
