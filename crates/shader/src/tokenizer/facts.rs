//! Reusable semantic facts derived from typed tokens.

use std::ops::Range;

use smol_str::{SmolStr, ToSmolStr};

use super::{
    AssignmentOperator, CallFact, ExpressionFact, ExpressionShape, KeywordType, OperatorType,
    TokenCursor, TokenIndexRange, TypedToken, expression::CallArgumentRanges,
};
use crate::codegen::FunctionParameterQualifier;

/// Facts derived from a typed token stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypedTokenFacts {
    /// Declaration declarators sorted by name token index.
    declarations: Vec<DeclarationFact>,
    /// Function-call-like ranges in source order.
    calls: Vec<CallFact>,
    /// Function signatures and prototypes in source order.
    function_signatures: Vec<FunctionSignatureFact>,
    /// Statements in source order.
    statements: Vec<StatementFact>,
    /// `for` statements in source order.
    for_loops: Vec<StatementFact>,
    /// Control-flow condition ranges in source order.
    conditions: Vec<ConditionFact>,
    /// Expression ranges in source order.
    expressions: Vec<ExpressionFact>,
}

impl TypedTokenFacts {
    /// Collects semantic facts from token storage.
    #[must_use]
    pub fn collect(tokens: TokenCursor<'_>) -> Self {
        let mut declarations = Vec::new();
        let mut function_signatures = FunctionSignatures {
            tokens,
            calls: Vec::new(),
            items: Vec::new(),
        };
        let mut expression_facts = ExpressionFacts::new(tokens);
        for range in function_signatures.collect_call_like_facts() {
            expression_facts.collect_range(TokenIndexRange::from_inclusive(range.0, range.1));
        }
        let mut statements = Vec::new();
        let mut conditions = Vec::new();
        let mut statement_facts = StatementFacts {
            tokens,
            statements: &mut statements,
            conditions: &mut conditions,
        };
        statement_facts.collect_all();
        statements.sort_by_key(|statement: &StatementFact| statement.range().start());
        conditions.sort_by_key(|condition: &ConditionFact| condition.range().start());
        let mut cursor = 0usize;
        while let Some((start, semicolon)) = tokens.next_semicolon_statement(&mut cursor) {
            Self::collect_statement(tokens, start, semicolon, &mut declarations);
            expression_facts.collect_statement(start, semicolon);
        }
        let mut header_cursor = 0usize;
        while let Some((open, close)) = tokens.next_for_loop_header(&mut header_cursor) {
            let Some(sections) =
                tokens.split_top_level_semicolon_sections(TokenIndexRange::new(open + 1, close))
            else {
                continue;
            };
            let Some(initializer) = sections
                .first()
                .copied()
                .filter(|section| !section.is_empty())
            else {
                continue;
            };
            Self::collect_statement(
                tokens,
                initializer.start(),
                initializer.end(),
                &mut declarations,
            );
            expression_facts.collect_range(initializer);
        }
        declarations.sort_by_key(DeclarationFact::name_index);
        let expressions = expression_facts.into_vec();
        let for_loops = statements
            .iter()
            .filter(|statement| matches!(statement.kind(), StatementKind::For { .. }))
            .cloned()
            .collect();
        let (calls, function_signatures) = function_signatures.into_parts();
        Self {
            declarations,
            calls,
            function_signatures,
            statements,
            for_loops,
            conditions,
            expressions,
        }
    }

    /// Collects declaration facts from one semicolon-terminated statement.
    fn collect_statement(
        tokens: TokenCursor<'_>,
        start: usize,
        semicolon: usize,
        declarations: &mut Vec<DeclarationFact>,
    ) {
        let Some(type_index) = DeclarationPrefix { tokens, start }.type_index() else {
            return;
        };
        let Some(ty) = tokens.get(type_index).and_then(|token| {
            matches!(
                token.kind(),
                TypedToken::TypeMark(_)
                    | TypedToken::Identifier(_)
                    | TypedToken::Keyword(KeywordType::Void)
            )
            .then(|| token.kind().source_text())
            .flatten()
        }) else {
            return;
        };
        let Some(name_index) = tokens.next_non_comment(type_index + 1) else {
            return;
        };
        if !matches!(
            tokens.get(name_index).map(super::Token::kind),
            Some(TypedToken::Identifier(_))
        ) {
            return;
        }
        if tokens
            .next_non_comment(name_index + 1)
            .is_some_and(|next| tokens[next].kind().is_left_paren())
        {
            return;
        }
        if tokens[type_index].kind().is_keyword(KeywordType::Struct)
            && tokens
                .next_non_comment(name_index + 1)
                .is_some_and(|next| matches!(tokens[next].kind(), TypedToken::LeftBrace))
        {
            return;
        }
        let statement = TokenIndexRange::new(start, semicolon + 1);
        declarations.append(
            &mut DeclarationDeclaratorFacts {
                tokens,
                ty: ty.into(),
                statement,
                type_index,
                name_index,
            }
            .collect(),
        );
    }

    /// Returns declaration declarator facts in source order.
    #[must_use]
    pub fn declarations(&self) -> &[DeclarationFact] {
        &self.declarations
    }

    /// Returns function-call-like facts in source order.
    #[must_use]
    pub fn calls(&self) -> &[CallFact] {
        &self.calls
    }

    /// Returns function signatures and prototypes in source order.
    #[must_use]
    pub fn function_signatures(&self) -> &[FunctionSignatureFact] {
        &self.function_signatures
    }

    /// Returns the function-call fact whose name starts at `name_index`.
    #[must_use]
    pub fn call_at_name(&self, name_index: usize) -> Option<&CallFact> {
        self.calls
            .binary_search_by_key(&name_index, CallFact::name_index)
            .ok()
            .map(|index| &self.calls[index])
    }

    /// Returns declaration facts whose statement starts at `statement_start`.
    #[must_use]
    pub fn declarations_at_statement_start(&self, statement_start: usize) -> &[DeclarationFact] {
        let start = self
            .declarations
            .partition_point(|declaration| declaration.statement().start() < statement_start);
        let end = self.declarations[start..]
            .partition_point(|declaration| declaration.statement().start() == statement_start);
        &self.declarations[start..start + end]
    }

    /// Returns the declaration fact whose name starts at `name_index`.
    #[must_use]
    pub fn declaration_at_name(&self, name_index: usize) -> Option<&DeclarationFact> {
        self.declarations
            .binary_search_by_key(&name_index, DeclarationFact::name_index)
            .ok()
            .map(|index| &self.declarations[index])
    }

    /// Returns statement facts in source order.
    #[must_use]
    pub fn statements(&self) -> &[StatementFact] {
        &self.statements
    }

    /// Returns `for` statement facts in source order.
    #[must_use]
    pub fn for_loops(&self) -> &[StatementFact] {
        &self.for_loops
    }

    /// Returns control-flow condition ranges in source order.
    #[must_use]
    pub fn conditions(&self) -> &[ConditionFact] {
        &self.conditions
    }

    /// Returns expression facts in source order.
    #[must_use]
    pub fn expressions(&self) -> &[ExpressionFact] {
        &self.expressions
    }

    /// Returns expression facts fully contained by `range`.
    pub fn expressions_contained(
        &self,
        range: TokenIndexRange,
    ) -> impl Iterator<Item = &ExpressionFact> {
        self.expressions.iter().filter(move |expression| {
            expression.range().start() >= range.start() && expression.range().end() <= range.end()
        })
    }

    /// Returns operand ranges from one binary expression tree whose top-level
    /// operators match `operators`, including nested binary operands from
    /// lower-precedence operand facts.
    #[must_use]
    pub fn binary_expression_operand_ranges(
        &self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
        operators: &[OperatorType],
    ) -> Vec<TokenIndexRange> {
        let mut operands = Vec::new();
        self.collect_binary_expression_operand_ranges(tokens, range, operators, &mut operands);
        operands.sort_by_key(|range| (range.start(), range.end()));
        operands.dedup();
        operands.sort_by_key(|range| range.start());
        operands
    }

    /// Recursively appends operand ranges from the expression tree rooted at
    /// `range`.
    fn collect_binary_expression_operand_ranges(
        &self,
        tokens: TokenCursor<'_>,
        range: TokenIndexRange,
        operators: &[OperatorType],
        operands: &mut Vec<TokenIndexRange>,
    ) {
        let Some(expression) = self.expression_covering(range.start()..range.end()) else {
            return;
        };
        let direct_operands = expression.binary_operand_ranges_for(operators);
        if direct_operands.is_empty() {
            if let ExpressionShape::Parenthesized(inner) =
                ExpressionShape::classify(tokens, expression.range())
            {
                self.collect_binary_expression_operand_ranges(tokens, inner, operators, operands);
            }
            return;
        }
        for operand in direct_operands {
            operands.push(operand);
            self.collect_binary_expression_operand_ranges(tokens, operand, operators, operands);
        }
    }

    /// Returns the expression fact covering `range`.
    #[must_use]
    pub fn expression_covering(&self, range: Range<usize>) -> Option<&ExpressionFact> {
        let start = self
            .expressions
            .partition_point(|expression| expression.range().start() < range.start);
        self.expressions[start..]
            .iter()
            .take_while(|expression| expression.range().start() == range.start)
            .find(|expression| expression.range().end() == range.end)
    }
}

/// Function signature fact collector.
struct FunctionSignatures<'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Collected call facts.
    calls: Vec<CallFact>,
    /// Collected signature facts.
    items: Vec<FunctionSignatureFact>,
}

impl FunctionSignatures<'_> {
    /// Collects call facts and function signature facts.
    fn collect_call_like_facts(&mut self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut call_index = 0usize;
        while call_index < self.tokens.len() {
            let name = match self.tokens[call_index].kind() {
                TypedToken::Identifier(name) => Some(name.clone()),
                TypedToken::TypeMark(primitive) => Some(primitive.text().to_smolstr()),
                kind if kind.is_keyword(KeywordType::For)
                    || kind.is_keyword(KeywordType::If)
                    || kind.is_keyword(KeywordType::Switch)
                    || kind.is_keyword(KeywordType::While) =>
                {
                    kind.source_text().map(SmolStr::new)
                }
                _ => None,
            };
            let Some(name) = name else {
                call_index += 1;
                continue;
            };
            let Some(open_index) = self.tokens.next_non_comment(call_index + 1) else {
                call_index += 1;
                continue;
            };
            if !self.tokens[open_index].kind().is_left_paren() {
                call_index += 1;
                continue;
            }
            let Some(close_index) = self.tokens.matching_right_paren(open_index) else {
                call_index += 1;
                continue;
            };
            self.calls.push(CallFact {
                name,
                name_index: call_index,
                open_index,
                close_index,
                arguments: CallArgumentRanges::new(self.tokens, open_index, close_index).into_vec(),
            });
            self.collect_call_like(call_index, open_index, close_index);
            ranges.push((call_index, close_index));
            call_index += 1;
        }
        ranges
    }

    /// Collects a function signature from a call-shaped token sequence.
    fn collect_call_like(&mut self, name_index: usize, open_index: usize, close_index: usize) {
        let TypedToken::Identifier(name) = self.tokens[name_index].kind() else {
            return;
        };
        if matches!(name.as_str(), "if" | "for" | "while" | "switch") {
            return;
        }
        let Some(after_close) = self.tokens.next_non_comment(close_index + 1) else {
            return;
        };
        if !matches!(
            self.tokens[after_close].kind(),
            TypedToken::Semicolon | TypedToken::LeftBrace
        ) {
            return;
        }
        let Some(return_type_index) = self.tokens.previous_non_comment(name_index) else {
            return;
        };
        let return_type = match self.tokens[return_type_index].kind() {
            TypedToken::Identifier(text) => text.as_str(),
            TypedToken::TypeMark(primitive) => primitive.text(),
            TypedToken::Keyword(KeywordType::Void) => KeywordType::Void.text(),
            _ => return,
        };
        let mut parameters = Vec::new();
        let mut start = open_index + 1;
        while start < close_index {
            let end = self.tokens.top_level_comma_segment_end(start, close_index);
            if let Some((start, end)) = self.tokens.non_comment_range(start, end) {
                let mut type_names = (start..=end).filter_map(|index| {
                    let kind = self.tokens[index].kind();
                    let text = kind.source_text()?;
                    (!FunctionParameterQualifier::is_token(kind)).then_some(text)
                });
                if let Some(parameter_type) = type_names.next()
                    && !(parameter_type == "void" && type_names.next().is_none())
                {
                    parameters.push(FunctionParameterFact {
                        ty: parameter_type.into(),
                    });
                }
            }
            start = end.saturating_add(1);
        }
        self.items.push(FunctionSignatureFact {
            name: name.clone(),
            return_type: return_type.into(),
            parameters,
        });
    }

    /// Returns collected call and signature facts in source order.
    fn into_parts(self) -> (Vec<CallFact>, Vec<FunctionSignatureFact>) {
        (self.calls, self.items)
    }
}

/// One function signature or prototype fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionSignatureFact {
    /// Function name.
    name: SmolStr,
    /// Return type spelling.
    return_type: SmolStr,
    /// Parameter facts in source order.
    parameters: Vec<FunctionParameterFact>,
}

impl FunctionSignatureFact {
    /// Returns the function name.
    #[must_use]
    pub const fn name(&self) -> &SmolStr {
        &self.name
    }

    /// Returns the declared return type.
    #[must_use]
    pub const fn return_type(&self) -> &SmolStr {
        &self.return_type
    }

    /// Returns parsed function parameter facts.
    #[must_use]
    pub fn parameters(&self) -> &[FunctionParameterFact] {
        &self.parameters
    }
}

/// One function parameter fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionParameterFact {
    /// Parameter type spelling.
    ty: SmolStr,
}

impl FunctionParameterFact {
    /// Returns the parameter type spelling.
    #[must_use]
    pub const fn ty(&self) -> &SmolStr {
        &self.ty
    }
}

/// Expression fact collector.
struct ExpressionFacts<'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Collected expression facts.
    expressions: Vec<ExpressionFact>,
}

impl<'tokens> ExpressionFacts<'tokens> {
    /// Starts collecting expression facts for `tokens`.
    #[must_use]
    #[allow(clippy::single_call_fn)]
    fn new(tokens: TokenCursor<'tokens>) -> Self {
        let mut facts = Self {
            tokens,
            expressions: Vec::new(),
        };
        facts.collect_range(TokenIndexRange::new(0, tokens.len()));
        facts
    }

    /// Collects expression facts from a semicolon statement.
    fn collect_statement(&mut self, start: usize, semicolon: usize) {
        if start >= semicolon {
            return;
        }
        self.collect_range(TokenIndexRange::new(start, semicolon));
        let range = TokenIndexRange::new(start, semicolon);
        let Some(expression) = ExpressionFact::collect(self.tokens, range) else {
            return;
        };
        let boundaries = expression
            .top_level_operators()
            .iter()
            .filter_map(|operator| {
                matches!(
                    operator.operator(),
                    OperatorType::Assignment(_)
                        | OperatorType::Conditional(_)
                        | OperatorType::Comma(_)
                )
                .then_some(operator.index())
            })
            .collect::<Vec<_>>();
        for operator in boundaries {
            let Some(expression_start) = self.tokens.next_non_comment(operator + 1) else {
                continue;
            };
            if expression_start >= semicolon {
                continue;
            }
            self.collect_range(TokenIndexRange::new(expression_start, semicolon));
        }
    }

    /// Collects one expression range and nested expression ranges.
    fn collect_range(&mut self, range: TokenIndexRange) {
        let Some(expression) = ExpressionFact::collect(self.tokens, range) else {
            return;
        };
        let range = expression.range();
        let operators = expression.top_level_operators().to_vec();
        let operands = expression.operand_ranges_for(&operators);
        let modulo_segments = expression.modulo_lowering_segments().to_vec();
        self.expressions.push(expression);
        if !operators.is_empty() {
            for operand in operands {
                if operand != range && !operand.is_empty() {
                    self.collect_range(operand);
                }
            }
        }
        for segment in modulo_segments {
            if segment != range && !segment.is_empty() {
                self.collect_range(segment);
            }
        }
        let mut index = range.start();
        while index < range.end() {
            let close = match self.tokens[index].kind() {
                kind if kind.is_left_paren() => self.tokens.matching_right_paren(index),
                kind if kind.is_left_square() => self.tokens.matching_right_square(index),
                _ => None,
            };
            let Some(close) = close.filter(|close| *close < range.end()) else {
                index += 1;
                continue;
            };
            if let Some(inner_start) = self.tokens.next_non_comment(index + 1)
                && let Some(inner_end) = self.tokens.previous_non_comment(close)
                && inner_start <= inner_end
            {
                self.collect_range(TokenIndexRange::from_inclusive(inner_start, inner_end));
                let mut start = inner_start;
                while start <= inner_end {
                    let end = self
                        .tokens
                        .top_level_comma_segment_end(start, inner_end + 1);
                    if let Some((segment_start, segment_end)) =
                        self.tokens.non_comment_range(start, end)
                    {
                        self.collect_range(TokenIndexRange::from_inclusive(
                            segment_start,
                            segment_end,
                        ));
                    }
                    if end > inner_end {
                        break;
                    }
                    start = end + 1;
                }
            }
            index = close + 1;
        }
    }

    /// Returns collected facts in source order.
    fn into_vec(mut self) -> Vec<ExpressionFact> {
        self.expressions
            .sort_by_key(|expression| expression.range().start());
        self.expressions
            .dedup_by_key(|expression| expression.range());
        self.expressions
    }
}

/// One declaration declarator fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationFact {
    /// Declared type spelling.
    ty: SmolStr,
    /// Declared name.
    name: SmolStr,
    /// Full semicolon-terminated declaration statement.
    statement: TokenIndexRange,
    /// This declarator's token range, including its separator.
    declarator: TokenIndexRange,
    /// Type token index.
    type_index: usize,
    /// Name token index.
    name_index: usize,
    /// Initializer expression token range.
    initializer: Option<TokenIndexRange>,
}

impl DeclarationFact {
    /// Returns the declared type spelling.
    #[must_use]
    pub const fn ty(&self) -> &SmolStr {
        &self.ty
    }

    /// Returns the declared name.
    #[must_use]
    pub const fn name(&self) -> &SmolStr {
        &self.name
    }

    /// Returns the full declaration statement range.
    #[must_use]
    pub const fn statement(&self) -> TokenIndexRange {
        self.statement
    }

    /// Returns this declarator's token range.
    #[must_use]
    pub const fn declarator(&self) -> TokenIndexRange {
        self.declarator
    }

    /// Returns the type token index.
    #[must_use]
    pub const fn type_index(&self) -> usize {
        self.type_index
    }

    /// Returns the name token index.
    #[must_use]
    pub const fn name_index(&self) -> usize {
        self.name_index
    }

    /// Returns the initializer token range.
    #[must_use]
    pub const fn initializer(&self) -> Option<TokenIndexRange> {
        self.initializer
    }

    /// Returns the first token after this declarator's initializer.
    #[must_use]
    pub const fn declarator_end(&self) -> usize {
        self.declarator.end()
    }
}

/// Declaration prefix parser.
#[derive(Clone, Copy)]
struct DeclarationPrefix<'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Candidate declaration start.
    start: usize,
}

impl DeclarationPrefix<'_> {
    /// Returns the token index of the declaration type.
    fn type_index(self) -> Option<usize> {
        let mut index = self.start;
        loop {
            let token = self.tokens.get(index)?;
            if token.kind().is_keyword(KeywordType::Layout) {
                index = self.after_layout(index)?;
                continue;
            }
            if !token.kind().is_declaration_modifier()
                && !matches!(
                    token.kind(),
                    TypedToken::Keyword(
                        KeywordType::Attribute
                            | KeywordType::Uniform
                            | KeywordType::Varying
                            | KeywordType::In
                            | KeywordType::Out
                    )
                )
            {
                return Some(index);
            }
            index = self.tokens.next_non_comment(index + 1)?;
        }
    }

    /// Returns the first token after a leading layout qualifier.
    fn after_layout(self, layout: usize) -> Option<usize> {
        let open = self.tokens.next_non_comment(layout + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return Some(open);
        }
        let close = self.tokens.matching_right_paren(open)?;
        self.tokens.next_non_comment(close + 1)
    }
}

/// Declarator fact collector for one declaration statement.
#[derive(Clone)]
struct DeclarationDeclaratorFacts<'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Shared type spelling.
    ty: SmolStr,
    /// Full declaration statement.
    statement: TokenIndexRange,
    /// Type token index.
    type_index: usize,
    /// Current declarator name index.
    name_index: usize,
}

impl DeclarationDeclaratorFacts<'_> {
    /// Collects declarators in the statement.
    fn collect(mut self) -> Vec<DeclarationFact> {
        let mut declarations = Vec::new();
        while self.name_index < self.statement.end().saturating_sub(1) {
            let Some(declaration) = self.declaration() else {
                break;
            };
            let next = self.next_name_after(declaration.declarator());
            declarations.push(declaration);
            let Some(next) = next else {
                break;
            };
            self.name_index = next;
        }
        declarations
    }

    /// Returns the current declarator fact.
    fn declaration(&self) -> Option<DeclarationFact> {
        let TypedToken::Identifier(name) = self.tokens[self.name_index].kind() else {
            return None;
        };
        let separator = self.separator_after_name()?;
        let initializer = self.initializer_before(separator);
        Some(DeclarationFact {
            ty: self.ty.clone(),
            name: name.clone(),
            statement: self.statement,
            declarator: TokenIndexRange::new(self.name_index, separator + 1),
            type_index: self.type_index,
            name_index: self.name_index,
            initializer,
        })
    }

    /// Returns the comma or semicolon ending this declarator.
    fn separator_after_name(&self) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.name_index + 1..self.statement.end() {
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                TypedToken::LeftBrace => brace_depth += 1,
                TypedToken::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Comma | TypedToken::Semicolon
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    return Some(index);
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the initializer expression range before `separator`.
    fn initializer_before(&self, separator: usize) -> Option<TokenIndexRange> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.name_index + 1..separator {
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                TypedToken::LeftBrace => brace_depth += 1,
                TypedToken::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Operator(OperatorType::Assignment(AssignmentOperator::Assign))
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    let start = self.tokens.next_non_comment(index + 1)?;
                    let end = self.tokens.previous_non_comment(separator)?;
                    return (start <= end).then_some(TokenIndexRange::new(start, end + 1));
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the next declarator name after the current declarator.
    fn next_name_after(&self, declarator: TokenIndexRange) -> Option<usize> {
        let separator = declarator.end().checked_sub(1)?;
        matches!(self.tokens[separator].kind(), TypedToken::Comma)
            .then(|| self.tokens.next_non_comment(separator + 1))
            .flatten()
            .filter(|name| {
                *name < self.statement.end()
                    && matches!(self.tokens[*name].kind(), TypedToken::Identifier(_))
            })
    }
}

/// One statement fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatementFact {
    /// Full statement token range.
    range: TokenIndexRange,
    /// Statement shape.
    kind: StatementKind,
}

impl StatementFact {
    /// Returns the full statement token range.
    #[must_use]
    pub const fn range(&self) -> TokenIndexRange {
        self.range
    }

    /// Returns the statement shape.
    #[must_use]
    pub const fn kind(&self) -> &StatementKind {
        &self.kind
    }

    /// Returns the `for` initializer range.
    #[must_use]
    pub const fn initializer(&self) -> Option<TokenIndexRange> {
        match self.kind {
            StatementKind::For { initializer, .. } => initializer,
            _ => None,
        }
    }

    /// Returns the control-flow condition range.
    #[must_use]
    pub const fn condition(&self) -> Option<TokenIndexRange> {
        match self.kind {
            StatementKind::If { condition, .. }
            | StatementKind::While { condition, .. }
            | StatementKind::DoWhile { condition, .. } => Some(condition),
            StatementKind::For { condition, .. } => condition,
            StatementKind::Simple => None,
        }
    }

    /// Returns the `for` step range.
    #[must_use]
    pub const fn step(&self) -> Option<TokenIndexRange> {
        match self.kind {
            StatementKind::For { step, .. } => step,
            _ => None,
        }
    }

    /// Returns the controlled body range.
    #[must_use]
    pub const fn body(&self) -> TokenIndexRange {
        match self.kind {
            StatementKind::If { body, .. }
            | StatementKind::For { body, .. }
            | StatementKind::While { body, .. }
            | StatementKind::DoWhile { body, .. } => body,
            StatementKind::Simple => self.range,
        }
    }

    /// Returns the optional `else` body range.
    #[must_use]
    pub const fn else_body(&self) -> Option<TokenIndexRange> {
        match self.kind {
            StatementKind::If { else_body, .. } => else_body,
            _ => None,
        }
    }
}

/// Statement shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatementKind {
    /// Semicolon-terminated or scope-only statement without a control header.
    Simple,
    /// `if (...)` with an optional `else` body.
    If {
        /// Condition expression.
        condition: TokenIndexRange,
        /// Then body.
        body: TokenIndexRange,
        /// Else body.
        else_body: Option<TokenIndexRange>,
    },
    /// `for (...; ...; ...)` loop.
    For {
        /// Initializer section.
        initializer: Option<TokenIndexRange>,
        /// Condition section.
        condition: Option<TokenIndexRange>,
        /// Step section.
        step: Option<TokenIndexRange>,
        /// Loop body.
        body: TokenIndexRange,
    },
    /// `while (...)` loop.
    While {
        /// Condition expression.
        condition: TokenIndexRange,
        /// Loop body.
        body: TokenIndexRange,
    },
    /// `do ... while (...)` loop.
    DoWhile {
        /// Loop body.
        body: TokenIndexRange,
        /// Condition expression.
        condition: TokenIndexRange,
    },
}

/// One control-flow condition fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionFact {
    /// Condition expression range.
    range: TokenIndexRange,
}

impl ConditionFact {
    /// Returns the condition expression range.
    #[must_use]
    pub const fn range(&self) -> TokenIndexRange {
        self.range
    }
}

/// Statement fact collector.
struct StatementFacts<'facts, 'tokens> {
    /// Token storage.
    tokens: TokenCursor<'tokens>,
    /// Collected statements.
    statements: &'facts mut Vec<StatementFact>,
    /// Collected condition ranges.
    conditions: &'facts mut Vec<ConditionFact>,
}

impl StatementFacts<'_, '_> {
    /// Collects all top-level and nested statements in source order.
    fn collect_all(&mut self) {
        let mut cursor = 0usize;
        while let Some(start) = self.tokens.next_non_comment(cursor) {
            let Some(end) = self.collect_at(start) else {
                cursor = start + 1;
                continue;
            };
            cursor = end.max(start + 1);
        }
    }

    /// Collects the statement at `start`, returning the first token after it.
    fn collect_at(&mut self, start: usize) -> Option<usize> {
        match self.tokens[start].kind() {
            TypedToken::LeftBrace => Some(self.collect_block(start)),
            TypedToken::RightBrace => Some(start + 1),
            kind if kind.is_keyword(KeywordType::If) => self.collect_if(start),
            kind if kind.is_keyword(KeywordType::For) => self.collect_for(start),
            kind if kind.is_keyword(KeywordType::While) => self.collect_while(start),
            kind if kind.is_keyword(KeywordType::Do) => self.collect_do_while(start),
            _ => self.collect_simple(start),
        }
    }

    /// Collects a braced block and its nested statements.
    fn collect_block(&mut self, open: usize) -> usize {
        let end = self.tokens.scope_end_after(open);
        let mut cursor = open + 1;
        while cursor < end.saturating_sub(1) {
            let Some(start) = self.tokens.next_non_comment(cursor) else {
                break;
            };
            if start >= end.saturating_sub(1) {
                break;
            }
            let Some(next) = self.collect_at(start) else {
                cursor = start + 1;
                continue;
            };
            cursor = next.max(start + 1);
        }
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(open, end),
            kind: StatementKind::Simple,
        });
        end
    }

    /// Collects an `if` statement.
    fn collect_if(&mut self, start: usize) -> Option<usize> {
        let open = self.tokens.next_non_comment(start + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.collect_simple(start);
        }
        let close = self.tokens.matching_right_paren(open)?;
        let condition = self.condition_range(open + 1, close)?;
        self.conditions.push(ConditionFact { range: condition });
        let body_start = self.tokens.next_non_comment(close + 1)?;
        let body_end = self.collect_at(body_start)?;
        let body = TokenIndexRange::new(body_start, body_end);
        let mut statement_end = body_end;
        let mut else_body = None;
        if let Some(else_index) = self.tokens.next_non_comment(body_end)
            && self.tokens[else_index].kind().is_keyword(KeywordType::Else)
        {
            let else_start = self.tokens.next_non_comment(else_index + 1)?;
            let else_end = self.collect_at(else_start)?;
            else_body = Some(TokenIndexRange::new(else_start, else_end));
            statement_end = else_end;
        }
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(start, statement_end),
            kind: StatementKind::If {
                condition,
                body,
                else_body,
            },
        });
        Some(statement_end)
    }

    /// Collects a `for` statement.
    fn collect_for(&mut self, start: usize) -> Option<usize> {
        let open = self.tokens.next_non_comment(start + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.collect_simple(start);
        }
        let close = self.tokens.matching_right_paren(open)?;
        let sections = self
            .tokens
            .split_top_level_semicolon_sections(TokenIndexRange::new(open + 1, close))?;
        let body_start = self.tokens.next_non_comment(close + 1)?;
        let body_end = self.collect_at(body_start)?;
        let initializer = sections
            .first()
            .copied()
            .and_then(|section| self.trim(section));
        let condition = sections
            .get(1)
            .copied()
            .and_then(|section| self.trim(section));
        let step = sections
            .get(2)
            .copied()
            .and_then(|section| self.trim(section));
        if let Some(condition) = condition {
            self.conditions.push(ConditionFact { range: condition });
        }
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(start, body_end),
            kind: StatementKind::For {
                initializer,
                condition,
                step,
                body: TokenIndexRange::new(body_start, body_end),
            },
        });
        Some(body_end)
    }

    /// Collects a `while` statement.
    fn collect_while(&mut self, start: usize) -> Option<usize> {
        let open = self.tokens.next_non_comment(start + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.collect_simple(start);
        }
        let close = self.tokens.matching_right_paren(open)?;
        let condition = self.condition_range(open + 1, close)?;
        self.conditions.push(ConditionFact { range: condition });
        let body_start = self.tokens.next_non_comment(close + 1)?;
        let body_end = self.collect_at(body_start)?;
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(start, body_end),
            kind: StatementKind::While {
                condition,
                body: TokenIndexRange::new(body_start, body_end),
            },
        });
        Some(body_end)
    }

    /// Collects a `do ... while (...)` statement.
    fn collect_do_while(&mut self, start: usize) -> Option<usize> {
        let body_start = self.tokens.next_non_comment(start + 1)?;
        let body_end = self.collect_at(body_start)?;
        let while_index = self.tokens.next_non_comment(body_end)?;
        if !self.tokens[while_index]
            .kind()
            .is_keyword(KeywordType::While)
        {
            self.statements.push(StatementFact {
                range: TokenIndexRange::new(start, body_end),
                kind: StatementKind::Simple,
            });
            return Some(body_end);
        }
        let open = self.tokens.next_non_comment(while_index + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.collect_simple(while_index);
        }
        let close = self.tokens.matching_right_paren(open)?;
        let condition = self.condition_range(open + 1, close)?;
        self.conditions.push(ConditionFact { range: condition });
        let semicolon = self.tokens.next_non_comment(close + 1)?;
        if !matches!(self.tokens[semicolon].kind(), TypedToken::Semicolon) {
            return None;
        }
        let end = semicolon + 1;
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(start, end),
            kind: StatementKind::DoWhile {
                body: TokenIndexRange::new(body_start, body_end),
                condition,
            },
        });
        Some(end)
    }

    /// Collects a simple statement.
    fn collect_simple(&mut self, start: usize) -> Option<usize> {
        let end = self.tokens.controlled_statement_end_after(start)?;
        self.collect_nested_blocks(start, end);
        self.statements.push(StatementFact {
            range: TokenIndexRange::new(start, end),
            kind: StatementKind::Simple,
        });
        Some(end)
    }

    /// Collects braced statement blocks nested inside a simple range, such as
    /// function bodies.
    fn collect_nested_blocks(&mut self, start: usize, end: usize) {
        let mut cursor = start;
        while cursor < end {
            let Some(open) = (cursor..end).find(|index| {
                matches!(self.tokens[*index].kind(), TypedToken::LeftBrace)
                    && self
                        .tokens
                        .matching_right_brace(*index)
                        .is_some_and(|close| close < end)
            }) else {
                break;
            };
            cursor = self.collect_block(open).max(open + 1);
        }
    }

    /// Returns a trimmed non-empty range.
    fn trim(&self, range: TokenIndexRange) -> Option<TokenIndexRange> {
        let (start, end) = self.tokens.non_comment_range(range.start(), range.end())?;
        Some(TokenIndexRange::from_inclusive(start, end))
    }

    /// Returns a trimmed condition range.
    fn condition_range(&self, start: usize, end: usize) -> Option<TokenIndexRange> {
        self.trim(TokenIndexRange::new(start, end))
    }
}
