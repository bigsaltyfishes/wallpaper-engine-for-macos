//! Token-backed declaration and declarator scanners.

mod functions;
mod parameters;
mod scoped;
mod types;

pub use parameters::FunctionParameterQualifier;
pub use scoped::{LocalDeclaration, LocalDeclarationStart};
pub use types::{DeclarationDeclarators, DeclaratorInitializer, LocalTypeName};

use self::{
    functions::FunctionParameterDeclarations,
    parameters::FunctionParameterTypeMode,
    scoped::{ScopedDeclarationDeclarators, ScopedDeclarationStart, StructTypeNames},
    types::DeclarationTypeName,
};
use crate::lexer::Token;

/// Scoped declaration/type facts shared by legalization policies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScopedDeclarationFacts<'src> {
    /// Struct type names declared as `struct Name { ... };`.
    struct_names: Vec<&'src str>,
    /// Function-body parameters and local/global declarators in source order.
    declarations: Vec<ScopedDeclarationFact<'src>>,
}

/// Controls which declaration type names are collected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopedDeclarationTypeMode {
    /// Built-in scalar/vector/matrix types only.
    Builtins,
    /// Built-in types plus source-declared struct names.
    BuiltinsAndStructs,
    /// Any syntactic type identifier.
    Any,
}

/// Collection policy for scoped declaration facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScopedDeclarationFactsConfig {
    /// Type mode for function definition parameters.
    pub(crate) parameter_types: ScopedDeclarationTypeMode,
    /// Type mode for local and global declarations.
    pub(crate) local_types: ScopedDeclarationTypeMode,
}

/// One scoped declaration fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScopedDeclarationFact<'src> {
    /// Declared name.
    name: &'src str,
    /// Declared type spelling.
    ty: &'src str,
    /// First token where this binding can be referenced.
    visible_start: usize,
    /// First token outside this binding's lexical scope.
    scope_end: usize,
}

impl<'src> ScopedDeclarationFacts<'src> {
    /// Collects shared scoped declaration facts.
    pub(crate) fn from_tokens(
        tokens: &[Token<'src>],
        config: ScopedDeclarationFactsConfig,
    ) -> Self {
        let struct_names = StructTypeNames::from(tokens).collect::<Vec<_>>();
        let mut declarations = Vec::new();
        for parameter in
            FunctionParameterDeclarations::from_tokens(tokens, FunctionParameterTypeMode::Any)
        {
            if config
                .parameter_types
                .accepts(parameter.ty(), &struct_names)
            {
                declarations.push(ScopedDeclarationFact {
                    name: parameter.name(),
                    ty: parameter.ty(),
                    visible_start: parameter.visible_start(),
                    scope_end: parameter.scope_end(),
                });
            }
        }
        for index in 0..tokens.len() {
            let Some(declaration) = ScopedDeclarationStart {
                tokens,
                struct_names: &struct_names,
                type_mode: config.local_types,
                start: index,
            }
            .declaration() else {
                continue;
            };
            declarations.extend(ScopedDeclarationDeclarators {
                tokens,
                next: Some(declaration),
            });
        }
        declarations.sort_by_key(|declaration| declaration.visible_start);
        Self {
            struct_names,
            declarations,
        }
    }

    /// Returns source-declared struct type names.
    pub(crate) fn struct_names(&self) -> &[&'src str] {
        &self.struct_names
    }

    /// Returns collected scoped declarations.
    pub(crate) fn declarations(&self) -> &[ScopedDeclarationFact<'src>] {
        &self.declarations
    }
}

impl ScopedDeclarationTypeMode {
    /// Returns whether `name` is collected by this mode.
    fn accepts(self, name: &str, struct_names: &[&str]) -> bool {
        match self {
            Self::Builtins => DeclarationTypeName::from(name).is_builtin(),
            Self::BuiltinsAndStructs => {
                DeclarationTypeName::from(name).is_builtin() || struct_names.contains(&name)
            }
            Self::Any => true,
        }
    }
}

impl<'src> ScopedDeclarationFact<'src> {
    /// Returns the declared name.
    pub(crate) const fn name(self) -> &'src str {
        self.name
    }

    /// Returns the declared type spelling.
    pub(crate) const fn ty(self) -> &'src str {
        self.ty
    }

    /// Returns the first token where this binding can be referenced.
    pub(crate) const fn visible_start(self) -> usize {
        self.visible_start
    }

    /// Returns first token outside this binding's lexical scope.
    pub(crate) const fn scope_end(self) -> usize {
        self.scope_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ShaderStageKind, syntax::ShaderModule};

    #[test]
    fn scoped_declaration_facts_collect_struct_params_locals_and_ignore_prototypes() {
        let source = concat!(
            "struct Payload { float value; };\n",
            "float global_value;\n",
            "void proto(Payload proto_payload, float proto_scalar);\n",
            "float helper(Payload payload, UnknownPayload unknown, float scalar) {\n",
            "    Payload local_payload;\n",
            "    for (float i = 0.0; i < 1.0; i += 1.0) {\n",
            "        scalar += i;\n",
            "    }\n",
            "    return scalar;\n",
            "}\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
        let facts = ScopedDeclarationFacts::from_tokens(
            module.tokens(),
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );

        assert_eq!(facts.struct_names(), ["Payload"]);
        assert!(facts.declarations().iter().any(|fact| {
            fact.name() == "payload"
                && fact.ty() == "Payload"
                && fact.scope_end() > fact.visible_start()
        }));
        assert!(facts.declarations().iter().any(|fact| {
            fact.name() == "unknown"
                && fact.ty() == "UnknownPayload"
                && fact.scope_end() > fact.visible_start()
        }));
        assert!(facts.declarations().iter().any(|fact| {
            fact.name() == "local_payload"
                && fact.ty() == "Payload"
                && fact.scope_end() > fact.visible_start()
        }));
        assert!(
            facts
                .declarations()
                .iter()
                .any(|fact| fact.name() == "i" && fact.ty() == "float")
        );
        assert!(
            !facts
                .declarations()
                .iter()
                .any(|fact| fact.name() == "proto_payload" || fact.name() == "proto_scalar")
        );
    }
}
