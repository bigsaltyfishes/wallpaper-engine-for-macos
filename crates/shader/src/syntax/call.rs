//! Syntax facts for function-call-like token ranges.

use smol_str::SmolStr;

use crate::{
    SourceSpan,
    tokenizer::{CallFact, TokenCursor, TokenIndexRange, TypedToken},
};

/// Syntax-owned scanner for function-call-like token ranges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionCalls<'stream> {
    /// Tokens being scanned.
    tokens: TokenCursor<'stream>,
    /// Precomputed call facts in source order.
    calls: &'stream [CallFact],
    /// Optional token range to include.
    range: Option<TokenIndexRange>,
    /// Next call fact index to inspect.
    next_index: usize,
}

impl<'stream> FunctionCalls<'stream> {
    /// Creates a function-call iterator over precomputed call facts.
    #[must_use]
    pub const fn new(tokens: TokenCursor<'stream>, calls: &'stream [CallFact]) -> Self {
        Self {
            tokens,
            calls,
            range: None,
            next_index: 0,
        }
    }

    /// Restricts returned calls to those fully contained in `range`.
    #[must_use]
    pub const fn in_range(mut self, range: TokenIndexRange) -> Self {
        self.range = Some(range);
        self
    }
}

impl Iterator for FunctionCalls<'_> {
    type Item = FunctionCall;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(fact) = self.calls.get(self.next_index) {
            self.next_index += 1;
            if self.range.is_some_and(|range| {
                fact.name_index() < range.start() || fact.close_index() >= range.end()
            }) {
                continue;
            }
            return FunctionCall::from_fact(self.tokens, fact);
        }
        None
    }
}

/// Token range for one syntactic function call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionCall {
    /// Function name text.
    name: SmolStr,
    /// Function name source span.
    name_span: SourceSpan,
    /// Source span covering the call expression.
    span: SourceSpan,
    /// Top-level call arguments.
    pub arguments: CallArguments,
    /// First top-level argument, when present.
    first_argument: Option<FirstCallArgument>,
    /// Whether this call is immediately followed by a field swizzle.
    has_trailing_swizzle: bool,
    /// Index of the function name token.
    pub name_index: usize,
    /// Index of the opening parenthesis.
    pub open_index: usize,
    /// Index of the matching closing parenthesis.
    pub close_index: usize,
}

impl FunctionCall {
    /// Creates a function call from a pre-collected tokenizer call fact.
    #[must_use]
    pub fn from_fact(tokens: TokenCursor<'_>, fact: &CallFact) -> Option<Self> {
        let name_index = fact.name_index();
        let open_index = fact.open_index();
        let close_index = fact.close_index();
        let arguments = CallArguments::from_ranges(tokens, fact.arguments());
        let name_span = tokens[name_index].span();
        let first_argument = {
            let mut iter = arguments.iter();
            iter.next().map(|first| FirstCallArgument {
                start: first.start(),
                span: first.span(),
                remaining_span: iter.next().and_then(|second| {
                    let end = tokens.previous_non_comment(close_index)?;
                    SourceSpan::new(
                        tokens[second.start()].span().start(),
                        tokens[end].span().end(),
                    )
                    .ok()
                }),
            })
        };

        let has_trailing_swizzle = tokens
            .next_non_comment(close_index + 1)
            .filter(|dot| tokens[*dot].kind().is_member_access_operator())
            .and_then(|dot| tokens.next_non_comment(dot + 1))
            .is_some_and(|field| matches!(tokens[field].kind(), TypedToken::Identifier(_)));

        Some(Self {
            name: fact.name().into(),
            name_span,
            span: SourceSpan::new(name_span.start(), tokens[close_index].span().end()).ok()?,
            arguments,
            first_argument,
            has_trailing_swizzle,
            name_index,
            open_index,
            close_index,
        })
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the source span for the function name.
    #[must_use]
    pub const fn name_span(&self) -> SourceSpan {
        self.name_span
    }

    /// Returns the span covering the entire call expression.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Counts top-level call arguments, ignoring nested parentheses.
    #[must_use]
    pub const fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    /// Returns whether this call is immediately followed by a field swizzle.
    #[must_use]
    pub const fn has_trailing_swizzle(&self) -> bool {
        self.has_trailing_swizzle
    }

    /// Returns token boundaries for the first top-level call argument.
    #[must_use]
    pub const fn first_argument(&self) -> Option<FirstCallArgument> {
        self.first_argument
    }
}

/// Top-level argument ranges for one function call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallArguments {
    /// Arguments in source order.
    items: Vec<CallArgument>,
}

impl CallArguments {
    /// Builds top-level call arguments from already-collected token ranges.
    #[must_use]
    pub fn from_ranges(tokens: TokenCursor<'_>, ranges: &[TokenIndexRange]) -> Self {
        let items = ranges
            .iter()
            .copied()
            .filter_map(|range| {
                let end = range.last()?;
                let span = SourceSpan::new(
                    tokens[range.start()].span().start(),
                    tokens[end].span().end(),
                )
                .ok()?;
                Some(CallArgument { range, span })
            })
            .collect();

        Self { items }
    }

    /// Returns the argument at `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<CallArgument> {
        self.items.get(index).copied()
    }

    /// Iterates top-level arguments.
    pub fn iter(&self) -> impl Iterator<Item = CallArgument> + '_ {
        self.items.iter().copied()
    }

    /// Returns the number of parsed arguments.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the call has no parsed arguments.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One top-level function argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallArgument {
    /// Token range covering the argument.
    range: TokenIndexRange,
    /// Source span covering the argument.
    span: SourceSpan,
}

impl CallArgument {
    /// Creates an argument from possibly-comment-padded token bounds.
    #[must_use]
    pub fn trim_from_bounds(tokens: TokenCursor<'_>, start: usize, end: usize) -> Option<Self> {
        let (start, end) = tokens.non_comment_range(start, end)?;
        let span = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end()).ok()?;
        Some(Self {
            range: TokenIndexRange::from_inclusive(start, end),
            span,
        })
    }

    /// Returns the first token index.
    #[must_use]
    pub const fn start(self) -> usize {
        self.range.start()
    }

    /// Returns the last token index.
    ///
    /// # Panics
    ///
    /// Panics if this argument was constructed with an empty token range.
    #[must_use]
    pub fn end(self) -> usize {
        self.range
            .last()
            .expect("call argument ranges are non-empty")
    }

    /// Returns the source span covering this argument.
    #[must_use]
    pub const fn span(self) -> SourceSpan {
        self.span
    }

    /// Returns whether the argument is exactly one non-comment token.
    #[must_use]
    pub fn is_single_token(self) -> bool {
        self.range.len() == 1
    }
}

/// Token range for the first argument of a function call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstCallArgument {
    /// First token of the argument.
    start: usize,
    /// Source span covering the argument expression.
    span: SourceSpan,
    /// Source span for arguments after the first argument.
    remaining_span: Option<SourceSpan>,
}

impl FirstCallArgument {
    /// Returns the first argument start token.
    pub const fn start(self) -> usize {
        self.start
    }

    /// Returns the source span for the first argument.
    pub const fn argument_span(self) -> SourceSpan {
        self.span
    }

    /// Returns the source span for arguments after the first argument.
    pub const fn remaining_argument_span(self) -> Option<SourceSpan> {
        self.remaining_span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::TokenStream;

    #[test]
    fn function_call_arguments_carry_source_spans() {
        let stream = TokenStream::lex("mul(vec3(1.0), value.xy)").expect("source lexes");
        let facts = stream.facts();
        let call = FunctionCalls::new(stream.cursor(), facts.calls())
            .next()
            .expect("call is indexed");
        let first = call.first_argument().expect("first argument exists");

        assert_eq!(call.name(), "mul");
        assert_eq!(call.argument_count(), 2);
        assert_eq!(
            first.argument_span(),
            SourceSpan::new(4, 13).expect("valid span")
        );
        assert_eq!(
            first.remaining_argument_span(),
            Some(SourceSpan::new(15, 23).expect("valid span"))
        );
    }

    #[test]
    fn comma_only_function_call_has_no_arguments() {
        let stream = TokenStream::lex("foo(,)").expect("source lexes");
        let facts = stream.facts();
        let call = FunctionCalls::new(stream.cursor(), facts.calls())
            .next()
            .expect("call is indexed");

        assert_eq!(call.name(), "foo");
        assert_eq!(call.argument_count(), 0);
    }

    #[test]
    fn repeated_comma_only_function_call_has_no_arguments() {
        let stream = TokenStream::lex("foo(,,)").expect("source lexes");
        let facts = stream.facts();
        let call = FunctionCalls::new(stream.cursor(), facts.calls())
            .next()
            .expect("call is indexed");

        assert_eq!(call.name(), "foo");
        assert_eq!(call.argument_count(), 0);
    }

    #[test]
    fn function_call_counts_multiple_real_arguments() {
        let stream = TokenStream::lex("foo(a, b, c)").expect("source lexes");
        let facts = stream.facts();
        let call = FunctionCalls::new(stream.cursor(), facts.calls())
            .next()
            .expect("call is indexed");

        assert_eq!(call.name(), "foo");
        assert_eq!(call.argument_count(), 3);
    }

    #[test]
    fn function_calls_continue_after_unmatched_candidate() {
        let stream = TokenStream::lex("broken(1.0; later(2.0)").expect("source lexes");
        let facts = stream.facts();
        let calls = FunctionCalls::new(stream.cursor(), facts.calls()).collect::<Vec<_>>();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name(), "later");
    }
}
