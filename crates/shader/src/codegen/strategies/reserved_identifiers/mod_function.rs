use super::{ScalarTypeFacts, ShaderModule, SourceSpan, SyntaxItem};

/// User `mod` collision plus scalar facts for call classification.
pub(super) struct ClassifiedModCollision {
    /// Parsed collision class.
    pub collision: ModCollision,
    /// Known scalar variable declarations.
    pub scalar_facts: ScalarTypeFacts,
}
/// Parsed source classes that need user `mod` collision rewrites.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ModCollision {
    /// Source declares `mod` with one argument.
    pub has_unary: bool,
    /// Source declares `float mod(float, float)`.
    pub has_scalar_binary: bool,
    /// Declaration name spans included in the collision class.
    pub name_spans: Vec<SourceSpan>,
}
/// Source facts needed to classify user-defined `mod` declarations.
pub(super) struct ModCollisionClass<'module, 'src> {
    /// Parsed shader module.
    pub module: &'module ShaderModule<'src>,
    /// Fallback declaration entries from the declaration plan.
    pub fallback_functions: Vec<crate::codegen::FunctionEntry<'src>>,
}

impl ModCollisionClass<'_, '_> {
    /// Classifies parsed `mod` declarations using syntax tokens.
    pub(super) fn classify(self) -> ModCollision {
        let mut collision = ModCollision::default();
        for function in self.module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) if function.name() == "mod" => Some(function),
            _ => None,
        }) {
            let parameter_types = function.parameter_types().collect::<Vec<_>>();
            if parameter_types.len() == 1 {
                collision.has_unary = true;
                collision.name_spans.push(function.name_span());
            } else if function.return_type() == "float"
                && parameter_types.as_slice() == ["float", "float"]
            {
                collision.has_scalar_binary = true;
                collision.name_spans.push(function.name_span());
            }
        }

        if collision.name_spans.is_empty() {
            collision.name_spans = self
                .fallback_functions
                .into_iter()
                .map(|function| function.name_span)
                .collect::<Vec<_>>();
            collision.has_unary = !collision.name_spans.is_empty();
        }

        collision
    }
}
