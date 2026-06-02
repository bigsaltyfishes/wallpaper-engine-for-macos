use smol_str::SmolStr;

use crate::{
    ShaderResult, SourceSpan,
    codegen::Fixup,
    tokenizer::{
        AssignmentOperator,
        OperatorType::{Assignment, Relational},
        PrimitiveType, RelationalOperator, StatementFact, TokenCursor, TokenIndexRange, TypedToken,
    },
};

/// Integer `for` bounds requiring explicit casts.
pub(super) struct IntegerForLoopCast {
    /// Tokenizer `for` statement fact.
    pub statement: StatementFact,
}

impl IntegerForLoopCast {
    /// Emits fixups for integer init RHS and comparison RHS.
    pub(super) fn fixups(self, tokens: TokenCursor<'_>) -> ShaderResult<Vec<Fixup>> {
        let mut fixups = Vec::new();
        let Some(section) = self.statement.initializer() else {
            return Ok(fixups);
        };
        let Some(initializer) = IntegerLoopInitializer { section }.candidate(tokens) else {
            return Ok(fixups);
        };
        IntegerCastFixup::around(initializer.rhs).push_to(&mut fixups)?;
        if let Some(condition) = self.statement.condition()
            && let Some(span) = (IntegerLoopCondition { section: condition })
                .rhs_span(tokens, initializer.name.as_str())
        {
            IntegerCastFixup::around(span).push_to(&mut fixups)?;
        }
        Ok(fixups)
    }
}

/// Integer loop initializer input.
struct IntegerLoopInitializer {
    /// Initializer section.
    section: TokenIndexRange,
}

/// Parsed integer loop initializer candidate.
struct IntegerLoopInitializerCandidate {
    /// Loop variable name.
    name: SmolStr,
    /// Initializer RHS span.
    rhs: SourceSpan,
}

impl IntegerLoopInitializer {
    /// Returns the loop variable and RHS span when the initializer is `int name
    /// = expr`.
    fn candidate(self, tokens: TokenCursor<'_>) -> Option<IntegerLoopInitializerCandidate> {
        let search = tokens;
        let section = self.section;
        let ty = search.next_non_comment(section.start())?;
        if ty >= section.end()
            || !matches!(tokens[ty].kind(), TypedToken::TypeMark(PrimitiveType::Int))
        {
            return None;
        }
        let name = search.next_non_comment(ty + 1)?;
        let TypedToken::Identifier(name_text) = tokens[name].kind() else {
            return None;
        };
        if name >= section.end() {
            return None;
        }
        let equals = search.next_non_comment(name + 1)?;
        if equals >= section.end()
            || !matches!(
                tokens[equals].kind(),
                TypedToken::Operator(Assignment(AssignmentOperator::Assign))
            )
        {
            return None;
        }
        Some(IntegerLoopInitializerCandidate {
            name: name_text.clone(),
            rhs: search.range_span(search.next_non_comment(equals + 1)?, section.end())?,
        })
    }
}

/// Integer loop condition candidate.
struct IntegerLoopCondition {
    /// Condition section.
    section: TokenIndexRange,
}

impl IntegerLoopCondition {
    /// Returns the RHS span when the condition compares the integer loop
    /// variable.
    fn rhs_span(self, tokens: TokenCursor<'_>, loop_variable: &str) -> Option<SourceSpan> {
        let section = self.section;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in section.start()..section.end() {
            match tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Operator(Relational(
                    RelationalOperator::Less
                    | RelationalOperator::Greater
                    | RelationalOperator::LessEqual
                    | RelationalOperator::GreaterEqual,
                )) if paren_depth == 0 && bracket_depth == 0 => {
                    let search = tokens;
                    let lhs_start = search.next_non_comment(section.start())?;
                    let lhs_end = search.previous_non_comment(index)?;
                    if lhs_start != lhs_end
                        || !matches!(
                            tokens[lhs_start].kind(),
                            TypedToken::Identifier(name) if name == loop_variable
                        )
                    {
                        return None;
                    }
                    let rhs = search.next_non_comment(index + 1)?;
                    return search.range_span(rhs, section.end());
                }
                _ => {}
            }
        }
        None
    }
}

/// Insertion fixups that wrap a source span in an `int(...)` cast.
struct IntegerCastFixup {
    /// Source span being wrapped.
    span: SourceSpan,
}

impl IntegerCastFixup {
    /// Creates a cast fixup around `span`.
    const fn around(span: SourceSpan) -> Self {
        Self { span }
    }

    /// Appends the insertion fixups.
    fn push_to(self, fixups: &mut Vec<Fixup>) -> ShaderResult<()> {
        let start = SourceSpan::new(self.span.start(), self.span.start())?;
        let end = SourceSpan::new(self.span.end(), self.span.end())?;
        fixups.push(Fixup::insert(start, "int(".to_owned()));
        fixups.push(Fixup::insert(end, ")".to_owned()));
        Ok(())
    }
}
