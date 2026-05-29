//! Legalizer orchestration context.

use super::{
    LegalizedStageSource,
    declarations::DeclarationPlan,
    emission::SourceEmitter,
    fixups::{Fixup, FixupSet},
    policies,
    tokens::TokenView,
};
use crate::{ShaderDiagnostic, ShaderResult, SourceSpan, syntax::ShaderModule};

/// Mutable working state shared by all legalizer analysis phases.
pub struct LegalizationContext<'module, 'src> {
    /// Parsed shader module being legalized.
    pub(crate) module: &'module ShaderModule<'src>,
    /// Token view used for syntax-aware source edits.
    pub(crate) tokens: TokenView<'module, 'src>,
    /// Planned replacement declarations and emitted resources.
    pub(crate) declarations: DeclarationPlan<'src>,
    /// Non-overlapping source edits collected before final emission.
    pub(crate) fixups: FixupSet,
    /// Diagnostics accumulated during analysis.
    pub(crate) diagnostics: Vec<ShaderDiagnostic>,
}

impl LegalizationContext<'_, '_> {
    /// Runs semantic analysis and emits the final legalized source.
    pub(super) fn legalize(self) -> ShaderResult<LegalizedStageSource> {
        self.legalize_with_policy_order(policies::PolicyOrder::Natural)
    }

    /// Runs semantic analysis using the requested policy order and emits the
    /// final legalized source.
    pub(super) fn legalize_with_policy_order(
        mut self,
        policy_order: policies::PolicyOrder,
    ) -> ShaderResult<LegalizedStageSource> {
        SemanticAnalyzer {
            context: &mut self,
            policy_order,
        }
        .analyze()?;
        self.diagnostics.push(
            ShaderDiagnostic::new("shader legalized")
                .with_stage(self.module.stage())
                .with_pass("Legalizer")
                .with_span(self.module.source_span()?),
        );

        let source = SourceEmitter {
            module: self.module,
            declarations: self.declarations,
            fixups: self.fixups,
        }
        .emit()?;
        Ok(LegalizedStageSource::new(
            self.module.stage(),
            source,
            self.diagnostics.into_boxed_slice(),
        ))
    }
}

/// Applies syntax-aware semantic rewrites into the shared fixup set.
struct SemanticAnalyzer<'ctx, 'module, 'src> {
    /// Legalization state being populated by analysis phases.
    context: &'ctx mut LegalizationContext<'module, 'src>,
    /// General policy execution order.
    policy_order: policies::PolicyOrder,
}

impl SemanticAnalyzer<'_, '_, '_> {
    /// Runs all legalizer analysis phases in dependency order.
    fn analyze(&mut self) -> ShaderResult<()> {
        self.context.declarations.plan_layouts()?;
        self.mark_top_level_declarations()?;
        let policy_order = self.policy_order;
        let mut policy_context = policies::PolicyContext {
            context: self.context,
        };
        match policy_order {
            policies::PolicyOrder::Natural => {
                for policy in policies::GENERAL_POLICIES {
                    policy.emit(&mut policy_context)?;
                }
            }
            #[cfg(test)]
            policies::PolicyOrder::Reverse => {
                for policy in policies::GENERAL_POLICIES.iter().rev() {
                    policy.emit(&mut policy_context)?;
                }
            }
        }
        Ok(())
    }

    /// Removes declarations that will be re-emitted with explicit layouts.
    fn mark_top_level_declarations(&mut self) -> ShaderResult<()> {
        let source = self.context.module.source().as_str();
        for span in self.context.declarations.removed_declarations() {
            let mut start = span.start();
            while start > 0 && matches!(source.as_bytes()[start - 1], b' ' | b'\t' | b'\x0c') {
                start -= 1;
            }
            let mut end = span.end();
            while end < source.len() && source.as_bytes()[end].is_ascii_whitespace() {
                let byte = source.as_bytes()[end];
                end += 1;
                if byte == b'\n' {
                    break;
                }
            }
            self.context
                .fixups
                .push(Fixup::replace(SourceSpan::new(start, end)?, ""));
        }
        for span in self.context.declarations.removed_qualifiers(source) {
            let mut end = span.end();
            while end < source.len() && matches!(source.as_bytes()[end], b' ' | b'\t') {
                end += 1;
            }
            self.context
                .fixups
                .push(Fixup::replace(SourceSpan::new(span.start(), end)?, ""));
        }
        Ok(())
    }
}
