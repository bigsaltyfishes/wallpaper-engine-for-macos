use super::{ExpressionReplacement, SymbolType};

impl ExpressionReplacement {
    /// Emits a binary expression from two lowered operands.
    pub(super) fn binary(left: Self, operator: impl Into<String>, right: Self) -> Self {
        Self::new()
            .with_replacement(left)
            .with_text(operator)
            .with_replacement(right)
    }

    /// Coerces a known integer operand to float for arithmetic modulo lowering.
    #[must_use]
    pub(super) fn into_float_operand(self, ty: Option<SymbolType>) -> Self {
        if matches!(ty, Some(SymbolType::Int | SymbolType::Uint)) {
            Self::changed_text("float(")
                .with_replacement(self)
                .with_text(")")
        } else {
            self
        }
    }
}
