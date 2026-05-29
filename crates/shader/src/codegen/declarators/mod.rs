//! Token-backed declaration and declarator scanners.

mod functions;
pub mod parameters;
pub mod scoped;
pub mod types;

pub(crate) use parameters::FunctionParameterQualifier;
pub(crate) use scoped::LocalDeclaration;
use smol_str::SmolStr;
pub(crate) use types::DeclaratorInitializer;

use crate::{
    codegen::LegacyTypeName,
    syntax::ShaderModule,
    tokenizer::{KeywordType, TypedToken},
};

/// Scoped declaration/type facts shared by codegen strategies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScopedDeclarationFacts {
    /// Struct type names declared as `struct Name { ... };`.
    struct_names: Vec<SmolStr>,
    /// Function-body parameters and local/global declarators in source order.
    declarations: Vec<ScopedDeclarationFact>,
}

/// Controls which declaration type names are collected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScopedDeclarationTypeMode {
    /// Built-in scalar/vector/matrix types only.
    Builtins,
    /// Built-in types plus source-declared struct names.
    BuiltinsAndStructs,
    /// Any syntactic type identifier.
    Any,
}

/// Collection strategy for scoped declaration facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScopedDeclarationFactsConfig {
    /// Type mode for function definition parameters.
    pub parameter_types: ScopedDeclarationTypeMode,
    /// Type mode for local and global declarations.
    pub local_types: ScopedDeclarationTypeMode,
}

/// One scoped declaration fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScopedDeclarationFact {
    /// Declared name.
    name: SmolStr,
    /// Declared type spelling.
    ty: SmolStr,
    /// First token where this binding can be referenced.
    visible_start: usize,
    /// First token outside this binding's lexical scope.
    scope_end: usize,
}

impl ScopedDeclarationFacts {
    /// Collects shared scoped declaration facts from a durable cursor.
    #[must_use]
    pub(crate) fn collect(module: &ShaderModule<'_>, config: ScopedDeclarationFactsConfig) -> Self {
        let tokens = module.token_stream().cursor();
        let token_facts = module.token_facts();
        let struct_names = {
            let mut names = Vec::new();
            for struct_index in 0..tokens.len() {
                if !matches!(
                    tokens[struct_index].kind(),
                    TypedToken::Keyword(KeywordType::Struct)
                ) {
                    continue;
                }
                let Some(name_index) = tokens.next_non_comment(struct_index + 1) else {
                    continue;
                };
                let TypedToken::Identifier(name) = tokens[name_index].kind() else {
                    continue;
                };
                let Some(open) = tokens.next_non_comment(name_index + 1) else {
                    continue;
                };
                if matches!(tokens[open].kind(), TypedToken::LeftBrace) {
                    names.push(name.clone());
                }
            }
            names
        };
        let mut declarations = Vec::new();
        for function in module.functions().iter() {
            let Some(body) = tokens
                .contained_byte_range(function.body_span().start(), function.body_span().end())
            else {
                continue;
            };
            let visible_start = body.start().saturating_add(1);
            let scope_end = body.end().saturating_sub(1);

            for parameter in function.parameters() {
                let Some(name) = parameter.name() else {
                    continue;
                };
                if config
                    .parameter_types
                    .accepts(parameter.ty().as_str(), &struct_names)
                {
                    declarations.push(ScopedDeclarationFact {
                        name: name.clone(),
                        ty: parameter.ty().clone(),
                        visible_start,
                        scope_end,
                    });
                }
            }
        }
        for fact in token_facts.declarations() {
            if !config
                .local_types
                .accepts(fact.ty().as_str(), &struct_names)
            {
                continue;
            }
            let parameter_candidate = tokens
                .enclosing_left_paren_before(fact.statement().start())
                .and_then(|open| {
                    let close = tokens.matching_right_paren(open)?;
                    let function_name = tokens.previous_non_comment(open)?;
                    let TypedToken::Identifier(name) = tokens[function_name].kind() else {
                        return None;
                    };
                    if matches!(name.as_str(), "for" | "if" | "switch" | "while") {
                        return None;
                    }
                    let return_type = tokens.previous_non_comment(function_name)?;
                    let _return_type = tokens[return_type].kind().source_text()?;
                    let after_close = tokens.next_non_comment(close + 1)?;
                    matches!(
                        tokens[after_close].kind(),
                        TypedToken::Semicolon | TypedToken::LeftBrace
                    )
                    .then_some(())
                })
                .is_some();
            if parameter_candidate {
                continue;
            }
            let Some(declaration) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            declarations.push(ScopedDeclarationFact {
                name: declaration.name().into(),
                ty: declaration.ty().into(),
                visible_start: declaration.name_index() + 1,
                scope_end: declaration.scope_end(),
            });
        }
        declarations.sort_by_key(|declaration| declaration.visible_start);
        Self {
            struct_names,
            declarations,
        }
    }

    /// Returns source-declared struct type names.
    #[must_use]
    pub(crate) fn struct_names(&self) -> &[SmolStr] {
        &self.struct_names
    }

    /// Returns collected scoped declarations.
    #[must_use]
    pub(crate) fn declarations(&self) -> &[ScopedDeclarationFact] {
        &self.declarations
    }
}

impl ScopedDeclarationTypeMode {
    /// Returns whether `name` is collected by this mode.
    fn accepts(self, name: &str, struct_names: &[SmolStr]) -> bool {
        match self {
            Self::Builtins => LegacyTypeName::new(name).is_builtin(),
            Self::BuiltinsAndStructs => {
                LegacyTypeName::new(name).is_builtin()
                    || struct_names.iter().any(|struct_name| struct_name == name)
            }
            Self::Any => true,
        }
    }
}

impl ScopedDeclarationFact {
    /// Returns the declared name.
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the declared type spelling.
    pub(crate) fn ty(&self) -> &str {
        self.ty.as_str()
    }

    /// Returns the first token where this binding can be referenced.
    pub(crate) const fn visible_start(&self) -> usize {
        self.visible_start
    }

    /// Returns first token outside this binding's lexical scope.
    pub(crate) const fn scope_end(&self) -> usize {
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
        let facts = ScopedDeclarationFacts::collect(
            &module,
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

    #[test]
    fn scoped_declaration_facts_collect_array_parameter_name_before_identifier_bound() {
        let source = concat!(
            "const int COUNT = 4;\n",
            "float helper(float values[COUNT]) {\n",
            "    return values[0];\n",
            "}\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
        let facts = ScopedDeclarationFacts::collect(
            &module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );

        assert!(facts.declarations().iter().any(|fact| {
            fact.name() == "values"
                && fact.ty() == "float"
                && fact.scope_end() > fact.visible_start()
        }));
    }

    #[test]
    fn scoped_declaration_facts_ignore_unnamed_parameters() {
        let source = "float helper(float, float named) { return named; }\n";
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
        let facts = ScopedDeclarationFacts::collect(
            &module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );

        assert!(
            facts
                .declarations()
                .iter()
                .any(|fact| fact.name() == "named" && fact.ty() == "float")
        );
        assert_eq!(
            facts
                .declarations()
                .iter()
                .filter(|fact| fact.ty() == "float")
                .count(),
            1
        );
    }
}
