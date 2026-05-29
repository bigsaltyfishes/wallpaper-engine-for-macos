use super::{
    DeclarationDeclarators, Fixup, LocalDeclaration, LocalDeclarationStart, PolicyContext,
    SourceSpan, Token, TokenKind, TokenSearch,
    assignment::{AssignmentEquals, AssignmentLhs, LhsExpression, LhsWidth},
    initializer::StatementEnd,
    types::{
        BindingType, ExpressionWidthFacts, SourcePointExt, SwizzledTokenRange, TokenRange,
        TokenRangeFacts, VectorTypeBindings, VectorWidth,
    },
};

#[derive(Default)]
/// Binary expressions that mix `vec3` identifiers with `vec2` constructors.
pub(super) struct Vec3Vec2BinaryExpressions {
    /// Constructor operands that need widening.
    pub(super) items: Vec<Vec3Vec2BinaryExpression>,
}
impl<'src> From<(&[Token<'src>], &VectorTypeBindings<'src>)> for Vec3Vec2BinaryExpressions {
    fn from((tokens, facts): (&[Token<'src>], &VectorTypeBindings<'src>)) -> Self {
        let mut items = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if !matches!(token.kind, TokenKind::Punctuation('+' | '-')) {
                continue;
            }
            let Some(expression) = Vec3Vec2BinaryExpression::try_from(BinaryOperator {
                tokens,
                operator: index,
                facts,
            })
            .ok() else {
                continue;
            };
            items.push(expression);
        }
        Self { items }
    }
}
#[derive(Clone, Copy)]
/// Candidate binary `+`/`-` operator.
pub(super) struct BinaryOperator<'tokens, 'facts, 'src> {
    /// Token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Operator token index.
    pub(super) operator: usize,
    /// Vector declaration facts.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One `vec2` operand that needs widening in a binary expression.
pub(super) struct Vec3Vec2BinaryExpression {
    /// Full vec2 constructor span.
    pub(super) operand: SourceSpan,
}
impl TryFrom<BinaryOperator<'_, '_, '_>> for Vec3Vec2BinaryExpression {
    type Error = ();

    fn try_from(operator: BinaryOperator<'_, '_, '_>) -> Result<Self, Self::Error> {
        let search = TokenSearch::new(operator.tokens);
        let left = search.previous_non_comment(operator.operator).ok_or(())?;
        let right = search.next_non_comment(operator.operator + 1).ok_or(())?;
        if operator.is_vec3_identifier(left) {
            return SourceSpan::try_from(ExpressionOperand {
                tokens: operator.tokens,
                start: right,
            })
            .map(|operand| Self { operand });
        }
        if operator.is_vec3_identifier(right) {
            return SourceSpan::try_from(ExpressionOperand {
                tokens: operator.tokens,
                start: left,
            })
            .map(|operand| Self { operand });
        }
        Err(())
    }
}
impl BinaryOperator<'_, '_, '_> {
    /// Returns whether the token at `index` is visibly declared as `vec3`.
    pub(super) fn is_vec3_identifier(self, index: usize) -> bool {
        let TokenKind::Identifier(name) = self.tokens[index].kind else {
            return false;
        };
        matches!(
            self.facts.lookup(name, index),
            Some(BindingType::Vector(VectorWidth::Three))
        )
    }
}
impl Vec3Vec2BinaryExpression {
    /// Emits constructor insertion fixups for this binary operand.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        context.context().fixups.push(Fixup::insert(
            self.operand.start_point(),
            "vec3(".to_owned(),
        ));
        context
            .context()
            .fixups
            .push(Fixup::insert(self.operand.end_point(), ", 0.0)".to_owned()));
    }
}
#[derive(Default)]
/// Vector binary expressions whose wider operands need target-width swizzles.
pub(super) struct VectorBinaryExpressions {
    /// Swizzle insertions in source order.
    pub(super) items: Vec<VectorBinaryExpression>,
}
impl<'src> From<(&[Token<'src>], &VectorTypeBindings<'src>)> for VectorBinaryExpressions {
    fn from((tokens, facts): (&[Token<'src>], &VectorTypeBindings<'src>)) -> Self {
        let mut items = Vec::new();
        for index in 0..tokens.len() {
            let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                tokens,
                start: index,
            }) else {
                continue;
            };
            let Some(target_width) = VectorWidth::from_constructor(declaration.ty()) else {
                continue;
            };
            for declaration in DeclarationDeclarators::new(tokens, declaration) {
                let Some(initializer) = declaration.initializer(tokens) else {
                    continue;
                };
                items.extend(
                    BinaryExpressionSwizzles {
                        tokens,
                        start: initializer.start(),
                        end: initializer.end(),
                        target_width,
                        facts,
                    }
                    .unique_items(&items),
                );
            }
        }
        for (index, token) in tokens.iter().enumerate() {
            if !matches!(token.kind, TokenKind::Punctuation('=')) {
                continue;
            }
            let Ok(lhs) = AssignmentLhs::try_from(AssignmentEquals {
                tokens,
                equals: index,
            }) else {
                continue;
            };
            let Some(target_width) = LhsWidth::try_from(LhsExpression {
                tokens,
                end: lhs.end,
                facts,
            })
            .ok()
            .map(|width| width.width) else {
                continue;
            };
            let search = TokenSearch::new(tokens);
            let Some(rhs_start) = search.next_non_comment(index + 1) else {
                continue;
            };
            let Some(statement_end) = (StatementEnd {
                tokens,
                start: rhs_start,
            })
            .semicolon() else {
                continue;
            };
            let Some(rhs_end) = search.previous_non_comment(statement_end) else {
                continue;
            };
            items.extend(
                BinaryExpressionSwizzles {
                    tokens,
                    start: rhs_start,
                    end: rhs_end,
                    target_width,
                    facts,
                }
                .unique_items(&items),
            );
        }
        Self { items }
    }
}
#[derive(Clone, Copy)]
/// Scanner for binary operands in one expression range.
pub(super) struct BinaryExpressionSwizzles<'tokens, 'facts, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token in the expression.
    pub(super) start: usize,
    /// Last token in the expression.
    pub(super) end: usize,
    /// Required vector width.
    pub(super) target_width: VectorWidth,
    /// Vector declaration facts.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
}
impl IntoIterator for BinaryExpressionSwizzles<'_, '_, '_> {
    type Item = VectorBinaryExpression;
    type IntoIter = std::vec::IntoIter<VectorBinaryExpression>;

    fn into_iter(self) -> Self::IntoIter {
        self.items().into_iter()
    }
}
impl BinaryExpressionSwizzles<'_, '_, '_> {
    /// Returns fixups not already present in `existing`.
    pub(super) fn unique_items(
        self,
        existing: &[VectorBinaryExpression],
    ) -> Vec<VectorBinaryExpression> {
        self.items()
            .into_iter()
            .filter(|item| !existing.contains(item))
            .collect()
    }

    /// Returns fixups for wider operands in this expression.
    pub(super) fn items(self) -> Vec<VectorBinaryExpression> {
        let operators = self.operators();
        if operators.is_empty() {
            return Vec::new();
        }
        let operands = self.operands(&operators);
        let operand_widths = operands
            .iter()
            .map(|operand| {
                ExpressionWidth {
                    tokens: self.tokens,
                    range: *operand,
                    facts: self.facts,
                }
                .vector_width()
            })
            .collect::<Vec<_>>();
        if !operand_widths.contains(&Some(self.target_width)) {
            return Vec::new();
        }

        operands
            .into_iter()
            .zip(operand_widths)
            .filter_map(|(operand, width)| {
                width
                    .is_some_and(|width| width > self.target_width)
                    .then_some(VectorBinaryExpression {
                        insertion: self.tokens[operand.end].span.end_point(),
                        swizzle: self.target_width.component_swizzle(),
                    })
            })
            .collect()
    }

    /// Returns top-level binary operator indices in source order.
    pub(super) fn operators(self) -> Vec<usize> {
        let mut operators = Vec::new();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.start..=self.end {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::Punctuation('+' | '-' | '*' | '/' | '%')
                    if paren_depth == 0 && bracket_depth == 0 && !self.is_unary_sign(index) =>
                {
                    operators.push(index);
                }
                _ => {}
            }
        }
        operators
    }

    /// Returns simple operand ranges split by `operators`.
    pub(super) fn operands(self, operators: &[usize]) -> Vec<ExpressionRange> {
        let search = TokenSearch::new(self.tokens);
        let mut ranges = Vec::with_capacity(operators.len() + 1);
        let mut start_bound = self.start;
        for operator in operators {
            let Some(left_end) = search.previous_non_comment(*operator) else {
                return Vec::new();
            };

            let mut left_start = left_end;
            if let TokenKind::Identifier(_field) = self.tokens[left_end].kind
                && let Some(dot) = search.previous_non_comment(left_end)
                && matches!(self.tokens[dot].kind, TokenKind::Punctuation('.'))
                && let Some(base) = search.previous_non_comment(dot)
            {
                left_start = base;
            }
            if left_start < start_bound {
                return Vec::new();
            }
            let left = ExpressionRange {
                start: left_start,
                end: left_end,
            };
            ranges.push(left);
            let Some(next_start) = search.next_non_comment(operator + 1) else {
                return Vec::new();
            };
            start_bound = next_start;
        }

        let mut last_end = start_bound;
        if let Some(dot) = search.next_non_comment(start_bound + 1)
            && matches!(self.tokens[dot].kind, TokenKind::Punctuation('.'))
            && let Some(field) = search.next_non_comment(dot + 1)
            && matches!(self.tokens[field].kind, TokenKind::Identifier(_))
        {
            last_end = field;
        }
        if last_end > self.end {
            return Vec::new();
        }
        let last = ExpressionRange {
            start: start_bound,
            end: last_end,
        };
        ranges.push(last);
        ranges
    }

    /// Returns whether a sign token is unary in this expression.
    pub(super) fn is_unary_sign(self, index: usize) -> bool {
        if !matches!(self.tokens[index].kind, TokenKind::Punctuation('+' | '-')) {
            return false;
        }
        let search = TokenSearch::new(self.tokens);
        let Some(previous) = search.previous_non_comment(index) else {
            return true;
        };
        previous < self.start
            || matches!(
                self.tokens[previous].kind,
                TokenKind::LeftParen
                    | TokenKind::Comma
                    | TokenKind::Punctuation(
                        '=' | '?' | ':' | '<' | '>' | '+' | '-' | '*' | '/' | '%'
                    )
            )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One wide vector binary operand that needs a trailing swizzle.
pub(super) struct VectorBinaryExpression {
    /// Insertion point immediately after the operand.
    pub(super) insertion: SourceSpan,
    /// Swizzle text to insert.
    pub(super) swizzle: &'static str,
}
impl VectorBinaryExpression {
    /// Emits the operand swizzle insertion.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, self.swizzle.to_owned()));
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Inclusive token range for a simple operand.
pub(super) struct ExpressionRange {
    /// First token.
    pub(super) start: usize,
    /// Last token.
    pub(super) end: usize,
}
impl TokenRangeFacts for ExpressionRange {
    fn start(self) -> usize {
        self.start
    }

    fn end(self) -> usize {
        self.end
    }
}
#[derive(Clone, Copy)]
/// Width classifier for a simple expression operand.
pub(super) struct ExpressionWidth<'tokens, 'facts, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Operand range.
    pub(super) range: ExpressionRange,
    /// Vector declaration facts.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
}
impl ExpressionWidth<'_, '_, '_> {
    /// Returns the vector width produced by this simple operand.
    pub(super) fn width(self) -> Option<VectorWidth> {
        if self.range.start == self.range.end {
            let TokenKind::Identifier(name) = self.tokens[self.range.start].kind else {
                return None;
            };
            return match self.facts.lookup(name, self.range.start) {
                Some(BindingType::Vector(width)) => Some(width),
                Some(BindingType::Scalar | BindingType::Blocker) | None => None,
            };
        }
        let swizzled = SwizzledTokenRange::try_from(TokenRange {
            tokens: self.tokens,
            start: self.range.start,
            end: self.range.end,
        })
        .ok()?;
        let TokenKind::Identifier(base) = self.tokens[swizzled.base].kind else {
            return None;
        };
        matches!(
            self.facts.lookup(base, swizzled.base),
            Some(BindingType::Vector(_))
        )
        .then_some(swizzled.width)
    }
}
impl ExpressionWidthFacts for ExpressionWidth<'_, '_, '_> {
    fn vector_width(self) -> Option<VectorWidth> {
        self.width()
    }
}
#[derive(Clone, Copy)]
/// Candidate expression operand start token.
pub(super) struct ExpressionOperand<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token in the operand.
    pub(super) start: usize,
}
impl TryFrom<ExpressionOperand<'_, '_>> for SourceSpan {
    type Error = ();

    fn try_from(operand: ExpressionOperand<'_, '_>) -> Result<Self, Self::Error> {
        let TokenKind::Identifier("vec2" | "float2" | "CAST2") = operand.tokens[operand.start].kind
        else {
            return Err(());
        };
        let open = TokenSearch::new(operand.tokens)
            .next_non_comment(operand.start + 1)
            .ok_or(())?;
        if !matches!(operand.tokens[open].kind, TokenKind::LeftParen) {
            return Err(());
        }
        let close = crate::legalizer::tokens::BalancedTokens::new(operand.tokens)
            .matching_right_paren(open)
            .ok_or(())?;
        SourceSpan::new(
            operand.tokens[operand.start].span.start(),
            operand.tokens[close].span.end(),
        )
        .map_err(|_error| ())
    }
}
