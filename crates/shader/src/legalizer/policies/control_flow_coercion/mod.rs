//! Control-flow scalar coercions accepted by Wallpaper Engine shaders.

/// Boolean-to-float declaration and compound-assignment coercions.
mod bool_coercion;
/// Shared expression classifiers.
mod expr;
/// Integer `for` loop bound casts.
mod for_bounds;
/// Integer declarations initialized by float expressions.
mod int_initializer;
/// `int` declarations initialized by `step`.
mod int_step;
/// Float modulo lowering.
mod modulo;
/// Statement-level token cursor.
mod statements;
/// Scoped scalar symbol facts.
mod symbols;

use linkme::distributed_slice;

use self::{
    bool_coercion::{BoolFloatInitializer, FloatTimesBool},
    expr::{NumericCondition, NumericConditionInput},
    for_bounds::{ForLoopHeaders, IntegerForLoopBounds},
    int_initializer::IntFloatInitializer,
    int_step::IntStepInitializer,
    modulo::FloatModulo,
    statements::StatementStream,
    symbols::SymbolFacts,
};
use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult,
    legalizer::{ExpressionReplacement, Fixup, FunctionCallIndex},
};

/// Rewrites C++-style scalar control-flow coercions to GLSL expressions.
struct ControlFlowCoercionPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static CONTROL_FLOW_COERCION_POLICY: &dyn Emitable = &ControlFlowCoercionPolicy;

impl Emitable for ControlFlowCoercionPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let module = context.context().module;
        let tokens = module.tokens();
        let facts = SymbolFacts::from(tokens);

        for statement in StatementStream::from(tokens) {
            for fixup in (FloatModulo {
                statement,
                facts: &facts,
            })
            .fixups()
            {
                context.context().fixups.push(fixup);
            }
            for fixup in (IntStepInitializer { statement }).fixups() {
                context.context().fixups.push(fixup);
            }
            for fixup in (IntFloatInitializer {
                statement,
                facts: &facts,
            })
            .fixups()
            {
                context.context().fixups.push(fixup);
            }
            for fixup in (BoolFloatInitializer {
                statement,
                facts: &facts,
            })
            .fixups()
            {
                context.context().fixups.push(fixup);
            }
            if let Ok(fixup) = Fixup::try_from(FloatTimesBool {
                statement,
                facts: &facts,
            }) {
                context.context().fixups.push(fixup);
            }
            for fixup in (NumericTernaryCondition {
                statement,
                facts: &facts,
            })
            .fixups()
            {
                context.context().fixups.push(fixup);
            }
        }

        for header in ForLoopHeaders::from(tokens) {
            for fixup in (IntegerForLoopBounds { header }).fixups()? {
                context.context().fixups.push(fixup);
            }
        }

        for condition in Conditions::from(tokens) {
            if let Ok(condition) = NumericCondition::try_from(NumericConditionInput {
                statement: condition.statement,
                start: condition.start,
                end: condition.end,
                facts: &facts,
            }) {
                let replacement = condition.replacement();
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(condition.span, replacement));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
/// Ternary `?:` condition expression that must be boolean in strict GLSL.
struct NumericTernaryCondition<'statement, 'tokens, 'src> {
    /// Statement being inspected.
    statement: statements::Statement<'tokens, 'src>,
    /// Known symbol facts.
    facts: &'statement SymbolFacts<'src>,
}

impl NumericTernaryCondition<'_, '_, '_> {
    /// Emits all numeric-to-boolean ternary condition fixups in this statement.
    fn fixups(self) -> Vec<Fixup> {
        TernaryConditions {
            tokens: self.statement.tokens,
            start: self.statement.start,
            end: self.statement.semicolon,
            cursor: self.statement.start,
        }
        .filter_map(|condition| {
            let condition = NumericCondition::try_from(NumericConditionInput {
                statement: self.statement,
                start: condition.start,
                end: condition.end,
                facts: self.facts,
            })
            .ok()?;
            Some(Fixup::replace(condition.span, condition.replacement()))
        })
        .collect()
    }
}

impl NumericCondition {
    /// Builds a numeric condition coercion while preserving child fixups inside
    /// the condition expression.
    fn replacement(self) -> ExpressionReplacement {
        ExpressionReplacement::new()
            .with_source(self.span)
            .with_text(" != ")
            .with_text(self.zero_literal())
    }
}

#[derive(Clone, Copy)]
/// Token range before a top-level ternary question mark.
struct TernaryCondition {
    /// First condition token.
    start: usize,
    /// Last condition token.
    end: usize,
}

/// Iterator over ternary condition ranges in one statement.
struct TernaryConditions<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [crate::lexer::Token<'src>],
    /// First token to inspect.
    start: usize,
    /// Statement semicolon token.
    end: usize,
    /// Scan cursor.
    cursor: usize,
}

impl Iterator for TernaryConditions<'_, '_> {
    type Item = TernaryCondition;

    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < self.end {
            let question = self.next_top_level_question(self.cursor)?;
            let start = self.condition_start(question)?;
            let end =
                crate::legalizer::TokenSearch::new(self.tokens).previous_non_comment(question)?;
            self.cursor = question + 1;
            if start <= end {
                return Some(TernaryCondition { start, end });
            }
        }
        None
    }
}

impl TernaryConditions<'_, '_> {
    /// Finds the next question mark not nested in delimiters.
    fn next_top_level_question(&self, start: usize) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().take(self.end).skip(start) {
            match token.kind {
                crate::lexer::TokenKind::LeftParen => paren_depth += 1,
                crate::lexer::TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                crate::lexer::TokenKind::Punctuation('[') => bracket_depth += 1,
                crate::lexer::TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                crate::lexer::TokenKind::Punctuation('?')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return Some(index);
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the first token of the condition immediately before `?`.
    fn condition_start(&self, question: usize) -> Option<usize> {
        let mut start = self.start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self
            .tokens
            .iter()
            .enumerate()
            .take(question)
            .skip(self.start)
        {
            match token.kind {
                crate::lexer::TokenKind::LeftParen => paren_depth += 1,
                crate::lexer::TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                crate::lexer::TokenKind::Punctuation('[') => bracket_depth += 1,
                crate::lexer::TokenKind::Punctuation(']') => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                crate::lexer::TokenKind::Punctuation('?' | ':')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    start = index + 1;
                }
                crate::lexer::TokenKind::Punctuation('=')
                    if paren_depth == 0
                        && bracket_depth == 0
                        && self.is_assignment_equals(index) =>
                {
                    start = index + 1;
                }
                _ => {}
            }
        }
        crate::legalizer::TokenSearch::new(self.tokens).next_non_comment(start)
    }

    /// Returns whether `=` is a standalone assignment separator rather than
    /// part of a comparison operator.
    fn is_assignment_equals(&self, index: usize) -> bool {
        let search = crate::legalizer::TokenSearch::new(self.tokens);
        !matches!(
            search
                .previous_non_comment(index)
                .map(|previous| self.tokens[previous].kind),
            Some(crate::lexer::TokenKind::Punctuation('=' | '!' | '<' | '>'))
        ) && !matches!(
            search
                .next_non_comment(index + 1)
                .map(|next| self.tokens[next].kind),
            Some(crate::lexer::TokenKind::Punctuation('='))
        )
    }
}

#[derive(Clone, Copy)]
/// Parenthesized condition expression inside an `if`, `while`, or `for` test.
struct Condition<'tokens, 'src> {
    /// Statement wrapper used by expression classifiers.
    statement: statements::Statement<'tokens, 'src>,
    /// First condition token.
    start: usize,
    /// Last condition token.
    end: usize,
}

/// Iterator over condition expressions that must be boolean in strict GLSL.
struct Conditions<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [crate::lexer::Token<'src>],
    /// Function-call-like parenthesized ranges.
    calls: std::vec::IntoIter<crate::legalizer::FunctionCall<'tokens, 'src>>,
}

impl<'tokens, 'src> From<&'tokens [crate::lexer::Token<'src>]> for Conditions<'tokens, 'src> {
    fn from(tokens: &'tokens [crate::lexer::Token<'src>]) -> Self {
        Self {
            tokens,
            calls: FunctionCallIndex::new(tokens)
                .iter()
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }
}

impl<'tokens, 'src> Iterator for Conditions<'tokens, 'src> {
    type Item = Condition<'tokens, 'src>;

    fn next(&mut self) -> Option<Self::Item> {
        for call in self.calls.by_ref() {
            if !matches!(call.name(), "if" | "while" | "for") {
                continue;
            }
            if call.name() == "for" {
                let tokens = call.tokens;
                let mut clauses = Vec::new();
                let mut start = call.open_index + 1;
                let mut paren_depth = 0usize;
                let mut bracket_depth = 0usize;
                for (index, token) in tokens
                    .iter()
                    .enumerate()
                    .take(call.close_index)
                    .skip(call.open_index + 1)
                {
                    match token.kind {
                        crate::lexer::TokenKind::LeftParen => paren_depth += 1,
                        crate::lexer::TokenKind::RightParen => {
                            paren_depth = paren_depth.checked_sub(1)?;
                        }
                        crate::lexer::TokenKind::Punctuation('[') => bracket_depth += 1,
                        crate::lexer::TokenKind::Punctuation(']') => {
                            bracket_depth = bracket_depth.checked_sub(1)?;
                        }
                        crate::lexer::TokenKind::Semicolon
                            if paren_depth == 0 && bracket_depth == 0 =>
                        {
                            clauses.push((start, index));
                            start = index + 1;
                        }
                        _ => {}
                    }
                }
                if clauses.len() != 2 {
                    continue;
                }
                let search = crate::legalizer::TokenSearch::new(tokens);
                let Some(start) = search.next_non_comment(clauses[1].0) else {
                    continue;
                };
                let Some(end) = search.previous_non_comment(clauses[1].1) else {
                    continue;
                };
                if start > end {
                    continue;
                }
                return Some(Condition {
                    statement: statements::Statement {
                        tokens: self.tokens,
                        start,
                        semicolon: end + 1,
                    },
                    start,
                    end,
                });
            }
            let start = crate::legalizer::TokenSearch::new(self.tokens)
                .next_non_comment(call.open_index + 1)?;
            let end = crate::legalizer::TokenSearch::new(self.tokens)
                .previous_non_comment(call.close_index)?;
            if start > end {
                continue;
            }
            return Some(Condition {
                statement: statements::Statement {
                    tokens: self.tokens,
                    start,
                    semicolon: call.close_index,
                },
                start,
                end,
            });
        }
        None
    }
}
