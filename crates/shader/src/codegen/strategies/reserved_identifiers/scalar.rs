use smol_str::SmolStr;

use crate::codegen::expressions::analysis::{ScalarExpressionFacts, ScalarType};

/// Known scoped declarations used to classify user `mod(float,float)` calls.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScalarTypeFacts {
    /// Declared scalar and blocker names in source order.
    pub bindings: Vec<ScalarBinding>,
}

impl ScalarTypeFacts {
    /// Returns whether a single identifier is known scalar.
    pub(super) fn contains(&self, name: &str, index: usize) -> bool {
        matches!(
            self.visible_type(name, index),
            Some(ScalarValueType::Scalar)
        )
    }

    /// Returns the nearest visible declaration type for `name` at `index`.
    pub(super) fn visible_type(&self, name: &str, index: usize) -> Option<ScalarValueType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(index))
            .map(|binding| binding.ty)
    }
}

impl ScalarExpressionFacts for ScalarTypeFacts {
    fn visible_type(&self, name: &str, index: usize) -> Option<ScalarType> {
        match self.visible_type(name, index) {
            Some(ScalarValueType::Scalar) => Some(ScalarType::Float),
            Some(ScalarValueType::NonScalar | ScalarValueType::Unknown) | None => None,
        }
    }

    fn scalar_identifier(&self, name: &str, index: usize) -> bool {
        self.contains(name, index)
    }
}

/// One scoped scalar or blocker binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScalarBinding {
    /// Declared name.
    pub name: SmolStr,
    /// Declared value type class.
    pub ty: ScalarValueType,
    /// First token where this binding can be referenced.
    pub visible_start: usize,
    /// First token outside the binding scope.
    pub scope_end: usize,
}

impl ScalarBinding {
    /// Returns whether this binding is visible at `index`.
    pub(super) const fn visible_at(&self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}
/// Type class for user `mod(float,float)` call classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScalarValueType {
    /// Float/int/uint/bool scalar.
    Scalar,
    /// Nearest declaration is known not to be a scalar value.
    NonScalar,
    /// Type is not known by this lightweight classifier.
    Unknown,
}

impl ScalarValueType {
    /// Classifies a declaration type spelling for scalar user `mod` routing.
    #[cfg(test)]
    fn classify_declared_type(ty: &str, struct_names: &[&str]) -> Self {
        match ty {
            "bool" | "int" | "uint" | "float" | "float1" => Self::Scalar,
            "float2" | "float3" | "float4" | "vec2" | "vec3" | "vec4" | "ivec2" | "ivec3"
            | "ivec4" | "uvec2" | "uvec3" | "uvec4" | "bvec2" | "bvec3" | "bvec4" | "mat2"
            | "mat3" | "mat4" | "mat2x2" | "mat2x3" | "mat2x4" | "mat3x2" | "mat3x3" | "mat3x4"
            | "mat4x2" | "mat4x3" | "mat4x4" => Self::NonScalar,
            _ if struct_names.contains(&ty) => Self::NonScalar,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_value_type_classifies_declared_types_and_struct_blockers() {
        assert_eq!(
            ScalarValueType::classify_declared_type("float", &["Material"]),
            ScalarValueType::Scalar
        );
        assert_eq!(
            ScalarValueType::classify_declared_type("vec3", &["Material"]),
            ScalarValueType::NonScalar
        );
        assert_eq!(
            ScalarValueType::classify_declared_type("Material", &["Material"]),
            ScalarValueType::NonScalar
        );
        assert_eq!(
            ScalarValueType::classify_declared_type("UnknownAlias", &["Material"]),
            ScalarValueType::Unknown
        );
    }
}
