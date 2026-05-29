use super::{
    DeclarationDeclarators, Fixup, LocalDeclaration, LocalDeclarationStart, PolicyContext,
    SourceSpan, Token, TokenKind, TokenSearch,
    initializer::StatementEnd,
    types::{
        BindingType, LegacyVectorTypeName, SourcePointExt, SwizzleField, VectorTypeBindings,
        VectorWidth,
    },
};

#[derive(Clone, Copy)]
/// Plain or compound assignment token.
pub(super) struct AssignmentEquals<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Assignment equals token index.
    pub(super) equals: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Left-hand side token range endpoint.
pub(super) struct AssignmentLhs {
    /// Last token of the left-hand side expression.
    pub(super) end: usize,
}
impl TryFrom<AssignmentEquals<'_, '_>> for AssignmentLhs {
    type Error = ();

    fn try_from(equals: AssignmentEquals<'_, '_>) -> Result<Self, Self::Error> {
        let search = TokenSearch::new(equals.tokens);
        let before_equals = search.previous_non_comment(equals.equals).ok_or(())?;
        match equals.tokens[before_equals].kind {
            TokenKind::Punctuation('=') => Err(()),
            TokenKind::Punctuation('+' | '-' | '*' | '/' | '%') => {
                let end = search.previous_non_comment(before_equals).ok_or(())?;
                Ok(Self { end })
            }
            _ => Ok(Self { end: before_equals }),
        }
    }
}
#[derive(Clone, Copy)]
/// Left-hand side expression whose vector width may constrain a call result.
pub(super) struct LhsExpression<'tokens, 'facts, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Last token of the expression.
    pub(super) end: usize,
    /// Known vector declarations.
    pub(super) facts: &'facts VectorTypeBindings<'src>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Vector width implied by a left-hand side expression.
pub(super) struct LhsWidth {
    /// Expected vector width.
    pub(super) width: VectorWidth,
}
impl TryFrom<LhsExpression<'_, '_, '_>> for LhsWidth {
    type Error = ();

    fn try_from(expression: LhsExpression<'_, '_, '_>) -> Result<Self, Self::Error> {
        if let TokenKind::Identifier(field) = expression.tokens[expression.end].kind
            && let Some(base) = expression.member_access_base()
        {
            if let Ok(field) = SwizzleField::parse(field)
                && expression.has_vector_base(base)
            {
                return Ok(Self { width: field.width });
            }
            return Err(());
        }

        let TokenKind::Identifier(name) = expression.tokens[expression.end].kind else {
            return Err(());
        };
        if let Some(width) = expression.declaration_width(name) {
            return Ok(Self { width });
        }
        if let Some(BindingType::Vector(width)) = expression.facts.lookup(name, expression.end) {
            return Ok(Self { width });
        }
        Err(())
    }
}
impl LhsExpression<'_, '_, '_> {
    /// Returns the base identifier index when the terminal field is a member
    /// access.
    pub(super) fn member_access_base(self) -> Option<usize> {
        let search = TokenSearch::new(self.tokens);
        let dot = search.previous_non_comment(self.end)?;
        if !matches!(self.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return None;
        }
        search.previous_non_comment(dot)
    }

    /// Returns whether `base` is a known vector expression.
    pub(super) fn has_vector_base(self, base: usize) -> bool {
        if self.is_chained_member_base(base) {
            return false;
        }
        let TokenKind::Identifier(base_name) = self.tokens[base].kind else {
            return false;
        };
        matches!(
            self.facts.lookup(base_name, base),
            Some(BindingType::Vector(_))
        )
    }

    /// Returns whether `base` is itself a field in a longer member chain.
    pub(super) fn is_chained_member_base(self, base: usize) -> bool {
        let search = TokenSearch::new(self.tokens);
        let Some(previous) = search.previous_non_comment(base) else {
            return false;
        };
        matches!(self.tokens[previous].kind, TokenKind::Punctuation('.'))
    }

    /// Returns the declared vector width when this LHS is a declarator name.
    pub(super) fn declaration_width(self, name: &str) -> Option<VectorWidth> {
        for start in (0..=self.end).rev() {
            let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                tokens: self.tokens,
                start,
            }) else {
                continue;
            };
            let Some(width) = VectorWidth::from_constructor(declaration.ty()) else {
                continue;
            };
            for declaration in DeclarationDeclarators::new(self.tokens, declaration) {
                if declaration.name_index() > self.end {
                    break;
                }
                if declaration.name_index() == self.end && declaration.name() == name {
                    return Some(width);
                }
            }
            break;
        }
        None
    }
}
/// Assignments whose right-hand side must match a narrowed vector target.
pub(super) struct NarrowVectorAssignments {
    /// Assignment RHS swizzle insertions in source order.
    pub(super) items: Vec<NarrowVectorAssignment>,
}
impl From<&mut PolicyContext<'_, '_, '_>> for NarrowVectorAssignments {
    fn from(context: &mut PolicyContext<'_, '_, '_>) -> Self {
        let state = context.context();
        let tokens = state.module.tokens();
        let mut items = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if !matches!(token.kind, TokenKind::Punctuation('=')) {
                continue;
            }

            let search = TokenSearch::new(tokens);
            let Some(lhs_end) = search.previous_non_comment(index) else {
                continue;
            };
            if matches!(
                tokens[lhs_end].kind,
                TokenKind::Punctuation('=' | '+' | '-' | '*' | '/' | '%')
            ) || search.next_non_comment(index + 1).is_none_or(|rhs_start| {
                matches!(tokens[rhs_start].kind, TokenKind::Punctuation('='))
            }) {
                continue;
            }

            let Ok(lhs) = AssignmentLhs::try_from(AssignmentEquals {
                tokens,
                equals: index,
            }) else {
                continue;
            };
            let lhs_width = if let TokenKind::Identifier(name) = tokens[lhs.end].kind {
                if let Some(ty) = state.declarations.stage_interface_ty(name) {
                    VectorWidth::from_constructor(LegacyVectorTypeName { ty }.glsl())
                } else {
                    let facts = VectorTypeBindings::from(tokens);
                    let expression = LhsExpression {
                        tokens,
                        end: lhs.end,
                        facts: &facts,
                    };
                    LhsWidth::try_from(expression).ok().map(|width| width.width)
                }
            } else {
                let facts = VectorTypeBindings::from(tokens);
                let expression = LhsExpression {
                    tokens,
                    end: lhs.end,
                    facts: &facts,
                };
                LhsWidth::try_from(expression).ok().map(|width| width.width)
            };
            let Some(lhs_width) = lhs_width else {
                continue;
            };

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
            if rhs_start > rhs_end {
                continue;
            }
            let rhs = AssignmentRhs {
                tokens,
                start: rhs_start,
                end: rhs_end,
            };
            if rhs.width().is_none_or(|rhs_width| rhs_width <= lhs_width) {
                continue;
            }
            let Some(swizzle) = lhs_width.narrow_swizzle() else {
                continue;
            };
            items.push(NarrowVectorAssignment {
                insertion: tokens[rhs.end].span.end_point(),
                swizzle,
            });
        }
        Self { items }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One assignment RHS that needs a trailing narrow swizzle.
pub(super) struct NarrowVectorAssignment {
    /// Source span immediately after the RHS expression.
    pub(super) insertion: SourceSpan,
    /// Swizzle text to insert.
    pub(super) swizzle: &'static str,
}
impl NarrowVectorAssignment {
    /// Emits the RHS narrowing swizzle insertion.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, self.swizzle.to_owned()));
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Assignment right-hand expression bounded by statement terminator.
pub(super) struct AssignmentRhs<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First non-comment RHS token.
    pub(super) start: usize,
    /// Last non-comment RHS token.
    pub(super) end: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// RHS expression with a terminal vector swizzle.
pub(super) struct SwizzledExpression {
    /// Width produced by the terminal swizzle.
    pub(super) width: VectorWidth,
}
impl TryFrom<AssignmentRhs<'_, '_>> for SwizzledExpression {
    type Error = ();

    fn try_from(rhs: AssignmentRhs<'_, '_>) -> Result<Self, Self::Error> {
        let TokenKind::Identifier(field) = rhs.tokens[rhs.end].kind else {
            return Err(());
        };
        let field = SwizzleField::parse(field)?;
        let dot = TokenSearch::new(rhs.tokens)
            .previous_non_comment(rhs.end)
            .ok_or(())?;
        matches!(rhs.tokens[dot].kind, TokenKind::Punctuation('.'))
            .then_some(Self { width: field.width })
            .ok_or(())
    }
}
