//! Codegen orchestration context.

use super::{
    CodegenStageSource,
    declarations::DeclarationPlan,
    emission::SourceEmitter,
    fixups::{Fixup, FixupSet},
    strategies,
};
use crate::{ShaderDiagnostic, ShaderResult, SourceSpan, syntax::ShaderModule};

/// Mutable working state shared by all legalizer analysis phases.
pub(super) struct CodegenContext<'module, 'src> {
    /// Parsed shader module being legalized.
    pub(super) module: &'module ShaderModule<'src>,
    /// Planned replacement declarations and emitted resources.
    pub(super) declarations: DeclarationPlan<'src>,
    /// Non-overlapping source edits collected before final emission.
    pub(super) fixups: FixupSet,
    /// Diagnostics accumulated during analysis.
    pub(super) diagnostics: Vec<ShaderDiagnostic>,
}

impl CodegenContext<'_, '_> {
    /// Runs semantic analysis and emits the final legalized source.
    pub(super) fn legalize(mut self) -> ShaderResult<CodegenStageSource> {
        SemanticAnalyzer { context: &mut self }.analyze()?;
        self.diagnostics.push(
            ShaderDiagnostic::new("shader legalized")
                .with_stage(self.module.stage())
                .with_pass("Codegen")
                .with_span(self.module.source_span()?),
        );

        let source = SourceEmitter {
            module: self.module,
            declarations: self.declarations,
            fixups: self.fixups,
        }
        .emit()?;
        Ok(CodegenStageSource::new(
            self.module.stage(),
            source,
            self.diagnostics.into_boxed_slice(),
        ))
    }
}

/// Applies syntax-aware semantic rewrites into the shared fixup set.
struct SemanticAnalyzer<'ctx, 'module, 'src> {
    /// Codegen state being populated by analysis phases.
    context: &'ctx mut CodegenContext<'module, 'src>,
}

impl SemanticAnalyzer<'_, '_, '_> {
    /// Runs all legalizer analysis phases in dependency order.
    fn analyze(&mut self) -> ShaderResult<()> {
        self.context.declarations.plan_layouts()?;
        self.mark_top_level_declarations()?;
        let mut strategy_context = strategies::StrategyContext {
            context: self.context,
        };
        strategy_context.emit_pipeline()?;
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
