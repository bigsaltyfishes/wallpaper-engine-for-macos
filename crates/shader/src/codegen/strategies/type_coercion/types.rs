use smol_str::SmolStr;

use super::ScopedDeclarationFacts;
use crate::{
    codegen::{
        DeclarationPlan,
        expressions::analysis::{
            FunctionCallRange, VectorExpressionAnalyzer, VectorExpressionFacts,
            VectorExpressionType, VectorWidth,
        },
    },
    tokenizer::{FunctionSignatureFact, TokenCursor, TypedTokenFacts},
};

/// Known scalar and vector declaration facts.
#[derive(Default)]
pub(super) struct VectorTypeBindings<'src> {
    /// Bindings in source order.
    pub bindings: Vec<TypeBinding>,
    /// Function return types in source order.
    pub functions: Vec<FunctionTypeBinding>,
    /// Source lifetime marker for declaration source slices that still come
    /// from syntax records.
    pub source: std::marker::PhantomData<&'src str>,
}

impl VectorTypeBindings<'_> {
    /// Builds type bindings from source-scoped declarations and generated
    /// top-level declarations planned by the legalizer.
    #[expect(
        clippy::single_call_fn,
        reason = "constructor keeps VectorTypeBindings assembly centralized"
    )]
    pub(super) fn new(
        scoped_facts: &ScopedDeclarationFacts,
        declarations: &DeclarationPlan<'_>,
    ) -> Self {
        let mut bindings = declarations
            .type_bindings()
            .map(|binding| TypeBinding {
                name: SmolStr::new(binding.name()),
                ty: BindingType::classify(binding.ty()),
                visible_start: 0,
                scope_end: usize::MAX,
            })
            .collect::<Vec<_>>();
        bindings.extend(Vec::from(scoped_facts));
        Self {
            bindings,
            functions: Vec::new(),
            source: std::marker::PhantomData,
        }
    }

    /// Looks up the nearest visible binding by name at `use_index`.
    pub(super) fn lookup(&self, name: &str, use_index: usize) -> Option<BindingType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(use_index))
            .map(|binding| binding.ty)
    }

    /// Resolves a declared function return type for a call expression.
    fn resolve_function_return(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        call: &FunctionCallRange,
    ) -> Option<BindingType> {
        let name = call.name.as_str();
        let mut matches = self
            .functions
            .iter()
            .rev()
            .filter(|function| function.name == name)
            .filter(|function| function.parameters.len() == call.arguments.len())
            .filter(|function| {
                function.parameters.iter().zip(call.arguments.iter()).all(
                    |(parameter, argument)| match parameter {
                        BindingType::Scalar => VectorExpressionAnalyzer {
                            facts: self,
                            token_facts,
                        }
                        .argument_is_scalar_like(tokens, argument),
                        BindingType::Vector(width) => {
                            VectorExpressionAnalyzer {
                                facts: self,
                                token_facts,
                            }
                            .argument_vector_width(tokens, argument)
                                == Some(*width)
                        }
                        BindingType::Blocker => false,
                    },
                )
            })
            .map(|function| function.ty);
        let first = matches.next()?;
        matches.all(|ty| ty == first).then_some(first)
    }
}

impl From<&ScopedDeclarationFacts> for Vec<TypeBinding> {
    fn from(facts: &ScopedDeclarationFacts) -> Self {
        facts
            .declarations()
            .iter()
            .map(|declaration| TypeBinding {
                name: SmolStr::new(declaration.name()),
                ty: BindingType::classify(declaration.ty()),
                visible_start: declaration.visible_start(),
                scope_end: declaration.scope_end(),
            })
            .collect()
    }
}

/// One scalar or vector binding.
#[derive(Clone)]
pub(super) struct TypeBinding {
    /// Variable name.
    pub name: SmolStr,
    /// Declared scalar or vector type.
    pub ty: BindingType,
    /// First token where this binding is visible.
    pub visible_start: usize,
    /// First token outside this binding's lexical scope.
    pub scope_end: usize,
}
/// Function return type fact.
#[derive(Clone)]
pub(super) struct FunctionTypeBinding {
    /// Function name.
    pub name: SmolStr,
    /// Declared return shape.
    pub ty: BindingType,
    /// Declared parameter shapes in source order.
    pub parameters: Vec<BindingType>,
}
/// Function return type facts collected from signatures and prototypes.
#[derive(Default)]
pub(super) struct FunctionTypeBindings {
    /// Function bindings in source order.
    pub items: Vec<FunctionTypeBinding>,
}

impl FunctionTypeBindings {
    /// Classifies tokenizer-owned function signature facts.
    pub(super) fn collect(&mut self, signatures: &[FunctionSignatureFact]) {
        self.items.extend(signatures.iter().filter_map(|fact| {
            let ty = BindingType::classify(fact.return_type().as_str());
            if matches!(ty, BindingType::Blocker) {
                return None;
            }
            Some(FunctionTypeBinding {
                name: fact.name().clone(),
                ty,
                parameters: fact
                    .parameters()
                    .iter()
                    .map(|parameter| BindingType::classify(parameter.ty().as_str()))
                    .collect(),
            })
        }));
    }
}

impl TypeBinding {
    /// Returns whether this binding is visible at `use_index`.
    pub(super) const fn visible_at(&self, use_index: usize) -> bool {
        self.visible_start <= use_index && use_index < self.scope_end
    }
}
/// Scalar/vector shape for one visible declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BindingType {
    /// Scalar or otherwise non-vector local type.
    Scalar,
    /// Vector declaration width.
    Vector(VectorWidth),
    /// Aggregate declaration that shadows an outer scalar/vector name.
    Blocker,
}

impl BindingType {
    /// Classifies declaration type spelling into scalar or vector shape.
    pub(super) fn classify(name: &str) -> Self {
        if let Some(width) = VectorWidth::classify_constructor(name) {
            Self::Vector(width)
        } else if matches!(
            name.as_bytes(),
            b"bool" | b"int" | b"uint" | b"float" | b"float1"
        ) {
            Self::Scalar
        } else {
            Self::Blocker
        }
    }
}

impl VectorExpressionFacts for VectorTypeBindings<'_> {
    fn expression_type(&self, name: &str, index: usize) -> Option<VectorExpressionType> {
        match self.lookup(name, index) {
            Some(BindingType::Scalar) => Some(VectorExpressionType::Scalar),
            Some(BindingType::Vector(width)) => Some(VectorExpressionType::Vector(width)),
            Some(BindingType::Blocker) => Some(VectorExpressionType::Blocker),
            None => None,
        }
    }

    fn function_return(
        &self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        call: &FunctionCallRange,
    ) -> Option<VectorExpressionType> {
        match self.resolve_function_return(tokens, token_facts, call)? {
            BindingType::Scalar => Some(VectorExpressionType::Scalar),
            BindingType::Vector(width) => Some(VectorExpressionType::Vector(width)),
            BindingType::Blocker => Some(VectorExpressionType::Blocker),
        }
    }
}
