//! Deterministic legalizer strategy pipeline.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use super::{
    CodegenStage, CodegenStrategy, CodegenStrategyName, GENERAL_POLICIES, StrategyContext,
};
use crate::ShaderResult;

/// Process-global legalizer pipeline built from distributed strategy metadata.
pub(super) static LEGALIZER_PIPELINE: LazyLock<CodegenPipeline> =
    LazyLock::new(|| CodegenPipeline::from_slice(&GENERAL_POLICIES));

/// Deterministically ordered legalizer strategy pipeline.
pub(super) struct CodegenPipeline {
    /// Ordered strategy references.
    ordered: Vec<CodegenStrategy>,
}

impl CodegenPipeline {
    /// Builds a pipeline from static strategy metadata.
    #[must_use]
    #[allow(clippy::single_call_fn)]
    pub(super) fn from_slice(strategies: &[CodegenStrategy]) -> Self {
        let graph = StrategyGraph::new(strategies);
        Self {
            ordered: graph.sorted(),
        }
    }

    /// Runs all strategies in deterministic dependency order.
    pub(super) fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        for strategy in &self.ordered {
            strategy.emitter.emit(context)?;
        }
        Ok(())
    }

    /// Returns ordered strategy names for tests and diagnostics.
    #[must_use]
    #[cfg(test)]
    fn ordered_names(&self) -> Vec<&'static str> {
        self.ordered
            .iter()
            .map(|strategy| strategy.name.as_str())
            .collect()
    }
}

/// Deterministic strategy dependency graph.
struct StrategyGraph {
    /// Strategies indexed by name.
    strategies: BTreeMap<CodegenStrategyName, CodegenStrategy>,
    /// Edges from dependency to dependent strategies.
    dependents: BTreeMap<CodegenStrategyName, BTreeSet<CodegenStrategyName>>,
    /// Number of unresolved dependencies for each strategy.
    incoming: BTreeMap<CodegenStrategyName, usize>,
}

impl StrategyGraph {
    /// Builds and validates a strategy graph.
    #[allow(clippy::single_call_fn)]
    fn new(strategies: &[CodegenStrategy]) -> Self {
        let mut indexed = BTreeMap::new();
        for strategy in strategies {
            assert!(
                indexed.insert(strategy.name, *strategy).is_none(),
                "duplicate legalizer strategy name: {}",
                strategy.name.as_str()
            );
        }

        let mut dependents: BTreeMap<CodegenStrategyName, BTreeSet<CodegenStrategyName>> =
            BTreeMap::new();
        let mut incoming = BTreeMap::new();
        for strategy in indexed.values() {
            let mut unique_dependencies = BTreeSet::new();
            let _ = incoming.insert(strategy.name, strategy.after.len());
            for dependency in strategy.after {
                assert!(
                    unique_dependencies.insert(*dependency),
                    "duplicate legalizer strategy dependency: {} after {}",
                    strategy.name.as_str(),
                    dependency.as_str()
                );
                let Some(dependency_strategy) = indexed.get(dependency) else {
                    panic!(
                        "missing legalizer strategy dependency: {} after {}",
                        strategy.name.as_str(),
                        dependency.as_str()
                    );
                };
                assert!(
                    dependency_strategy.stage <= strategy.stage,
                    "legalizer strategy dependency inverts stage order: {}({:?}) after {}({:?})",
                    strategy.name.as_str(),
                    strategy.stage,
                    dependency_strategy.name.as_str(),
                    dependency_strategy.stage
                );
                let _ = dependents
                    .entry(*dependency)
                    .or_default()
                    .insert(strategy.name);
            }
        }

        Self {
            strategies: indexed,
            dependents,
            incoming,
        }
    }

    /// Returns strategies in deterministic topological order.
    fn sorted(mut self) -> Vec<CodegenStrategy> {
        let mut ready = BTreeSet::new();
        for (name, count) in &self.incoming {
            if *count == 0 {
                let _ = ready.insert(
                    self.strategies
                        .get(name)
                        .expect("indexed strategy exists")
                        .sort_key(),
                );
            }
        }

        let mut ordered = Vec::with_capacity(self.strategies.len());
        while let Some(key) = ready.pop_first() {
            let strategy = self
                .strategies
                .get(&key.name)
                .copied()
                .expect("ready strategy exists");
            ordered.push(strategy);

            if let Some(dependents) = self.dependents.remove(&strategy.name) {
                for dependent in dependents {
                    let count = self
                        .incoming
                        .get_mut(&dependent)
                        .expect("dependent incoming count exists");
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        let dependent_strategy = self
                            .strategies
                            .get(&dependent)
                            .expect("dependent strategy exists");
                        let _ = ready.insert(dependent_strategy.sort_key());
                    }
                }
            }
        }

        if ordered.len() != self.strategies.len() {
            let unresolved = self
                .incoming
                .iter()
                .filter_map(|(name, count)| (*count > 0).then_some(name.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            panic!("legalizer strategy dependency cycle: {unresolved}");
        }

        ordered
    }
}

/// Deterministic ready-queue key.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StrategySortKey {
    /// Stage ordering.
    stage: CodegenStage,
    /// Strategy name ordering.
    name: CodegenStrategyName,
}

impl CodegenStrategy {
    /// Returns the deterministic ready-queue sort key.
    const fn sort_key(&self) -> StrategySortKey {
        StrategySortKey {
            stage: self.stage,
            name: self.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::strategies::Emitable;

    impl CodegenPipeline {
        fn from_slice_for_test(strategies: &[CodegenStrategy]) -> Self {
            Self::from_slice(strategies)
        }
    }

    struct TestEmitter;

    impl Emitable for TestEmitter {
        fn emit(&self, _context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
            Ok(())
        }
    }

    static TEST_EMITTER: TestEmitter = TestEmitter;

    fn strategy(
        name_value: &'static str,
        stage: CodegenStage,
        after: &'static [CodegenStrategyName],
    ) -> CodegenStrategy {
        CodegenStrategy {
            name: CodegenStrategyName::new(name_value),
            stage,
            after,
            emitter: &TEST_EMITTER,
        }
    }

    #[test]
    fn pipeline_order_is_independent_of_input_slice_order() {
        static AFTER_A: [CodegenStrategyName; 1] = [CodegenStrategyName::new("a")];
        let first = vec![
            strategy("b", CodegenStage::CompatibilityExpansion, &AFTER_A),
            strategy("a", CodegenStage::CompatibilityExpansion, &[]),
            strategy("c", CodegenStage::SemanticRewrite, &[]),
        ];
        let second = vec![
            strategy("c", CodegenStage::SemanticRewrite, &[]),
            strategy("a", CodegenStage::CompatibilityExpansion, &[]),
            strategy("b", CodegenStage::CompatibilityExpansion, &AFTER_A),
        ];

        let first_order = CodegenPipeline::from_slice_for_test(&first).ordered_names();
        let second_order = CodegenPipeline::from_slice_for_test(&second).ordered_names();

        assert_eq!(first_order, ["a", "b", "c"]);
        assert_eq!(first_order, second_order);
    }

    #[test]
    fn after_dependency_runs_current_strategy_later_than_dependency() {
        static AFTER_BASE: [CodegenStrategyName; 1] = [CodegenStrategyName::new("base")];
        let strategies = vec![
            strategy("current", CodegenStage::CompatibilityExpansion, &AFTER_BASE),
            strategy("base", CodegenStage::CompatibilityExpansion, &[]),
        ];

        let order = CodegenPipeline::from_slice_for_test(&strategies).ordered_names();

        assert_eq!(order, ["base", "current"]);
    }

    #[test]
    fn stage_order_breaks_ties_between_independent_strategies() {
        let strategies = vec![
            strategy("types", CodegenStage::TypeCodegen, &[]),
            strategy("compat", CodegenStage::CompatibilityExpansion, &[]),
            strategy("semantic", CodegenStage::SemanticRewrite, &[]),
        ];

        let order = CodegenPipeline::from_slice_for_test(&strategies).ordered_names();

        assert_eq!(order, ["compat", "semantic", "types"]);
    }

    #[test]
    #[should_panic(expected = "duplicate legalizer strategy name: dup")]
    fn duplicate_names_panic() {
        let strategies = vec![
            strategy("dup", CodegenStage::CompatibilityExpansion, &[]),
            strategy("dup", CodegenStage::SemanticRewrite, &[]),
        ];

        let _pipeline = CodegenPipeline::from_slice_for_test(&strategies);
    }

    #[test]
    #[should_panic(expected = "missing legalizer strategy dependency: current after missing")]
    fn missing_dependencies_panic() {
        static AFTER_MISSING: [CodegenStrategyName; 1] = [CodegenStrategyName::new("missing")];
        let strategies = vec![strategy(
            "current",
            CodegenStage::CompatibilityExpansion,
            &AFTER_MISSING,
        )];

        let _pipeline = CodegenPipeline::from_slice_for_test(&strategies);
    }

    #[test]
    #[should_panic(expected = "duplicate legalizer strategy dependency: current after base")]
    fn duplicate_dependencies_panic_with_dependency_name() {
        static AFTER_BASE_TWICE: [CodegenStrategyName; 2] = [
            CodegenStrategyName::new("base"),
            CodegenStrategyName::new("base"),
        ];
        let strategies = vec![
            strategy(
                "current",
                CodegenStage::CompatibilityExpansion,
                &AFTER_BASE_TWICE,
            ),
            strategy("base", CodegenStage::CompatibilityExpansion, &[]),
        ];

        let _pipeline = CodegenPipeline::from_slice_for_test(&strategies);
    }

    #[test]
    #[should_panic(expected = "legalizer strategy dependency inverts stage order")]
    fn stage_inverting_dependencies_panic() {
        static AFTER_LATE: [CodegenStrategyName; 1] = [CodegenStrategyName::new("late")];
        let strategies = vec![
            strategy("early", CodegenStage::CompatibilityExpansion, &AFTER_LATE),
            strategy("late", CodegenStage::TypeCodegen, &[]),
        ];

        let _pipeline = CodegenPipeline::from_slice_for_test(&strategies);
    }

    #[test]
    #[should_panic(expected = "legalizer strategy dependency cycle")]
    fn cycles_panic() {
        static AFTER_B: [CodegenStrategyName; 1] = [CodegenStrategyName::new("b")];
        static AFTER_A: [CodegenStrategyName; 1] = [CodegenStrategyName::new("a")];
        let strategies = vec![
            strategy("a", CodegenStage::CompatibilityExpansion, &AFTER_B),
            strategy("b", CodegenStage::CompatibilityExpansion, &AFTER_A),
        ];

        let _pipeline = CodegenPipeline::from_slice_for_test(&strategies);
    }

    #[test]
    fn production_pipeline_contains_each_strategy_once() {
        let names = LEGALIZER_PIPELINE.ordered_names();

        assert_eq!(names.len(), 13);
        let unique = names.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), names.len());
        assert_eq!(
            unique,
            [
                "alpha_to_coverage",
                "array_parameters",
                "compatibility_functions",
                "control_flow_coercion",
                "fragment_output",
                "hlsl_mul",
                "legacy_builtins",
                "legacy_types",
                "mutable_inputs",
                "reserved_identifiers",
                "scalar_texture",
                "texture_sampling",
                "type_coercion",
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn production_pipeline_order_is_stable() {
        let names = LEGALIZER_PIPELINE.ordered_names();

        assert_eq!(
            names,
            [
                "legacy_types",
                "texture_sampling",
                "legacy_builtins",
                "compatibility_functions",
                "hlsl_mul",
                "reserved_identifiers",
                "alpha_to_coverage",
                "array_parameters",
                "control_flow_coercion",
                "type_coercion",
                "scalar_texture",
                "mutable_inputs",
                "fragment_output",
            ]
        );
    }
}
