use super::{
    DeclaratorInitializer, Fixup, LocalDeclaration, SourceSpan, StrategyContext, TypedToken,
    calls::{CoercionArguments, CoercionFunction},
    types::{BindingType, VectorTypeBindings},
};
use crate::{
    codegen::expressions::analysis::{VectorExpressionAnalyzer, VectorWidth},
    syntax::{CallArgument, FunctionCall},
    tokenizer::{
        ArithmeticOperator, LiteralValue, OperatorType::Arithmetic, TokenCursor, TypedTokenFacts,
    },
};

/// Local vector declarations initialized from wider visible vector bindings.
pub(super) struct NarrowVectorInitializers<'facts> {
    /// Shared scoped declaration facts.
    pub facts: &'facts VectorTypeBindings<'facts>,
    /// Matching declarations found during the scan.
    pub items: Vec<NarrowVectorInitializer>,
}

impl NarrowVectorInitializers<'_> {
    /// Scans tokens in source order for vector declarations.
    pub(super) fn scan(&mut self, tokens: TokenCursor<'_>, token_facts: &TypedTokenFacts) {
        for fact in token_facts.declarations() {
            let Some(declaration) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            let Some(width) = VectorWidth::classify_constructor(declaration.ty()) else {
                continue;
            };
            self.collect_declaration(tokens, token_facts, &declaration, width);
        }
    }

    /// Records any required initializer swizzle for a vector declaration.
    pub(super) fn collect_declaration(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        declaration: &LocalDeclaration,
        width: VectorWidth,
    ) {
        let Some(initializer) = declaration.initializer(tokens) else {
            return;
        };

        self.collect_initializer(tokens, token_facts, width, initializer);
    }

    /// Emits a narrow-vector initializer swizzle when a wider vector
    /// expression is assigned.
    pub(super) fn collect_initializer(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        width: VectorWidth,
        initializer: DeclaratorInitializer,
    ) {
        let Some(swizzle) = width.narrow_swizzle() else {
            return;
        };
        let Some(initializer_width) = initializer.vector_width(tokens, token_facts, self.facts)
        else {
            return;
        };
        if initializer_width <= width {
            return;
        }
        if initializer.is_context_coerced_call(tokens, token_facts) {
            return;
        }
        if initializer.start() == initializer.end()
            && let Ok(insertion) = SourceSpan::new(
                tokens[initializer.start()].span().end(),
                tokens[initializer.start()].span().end(),
            )
        {
            self.items.push(NarrowVectorInitializer {
                span: insertion,
                swizzle,
                parenthesize: false,
            });
            return;
        }
        self.items.push(NarrowVectorInitializer {
            span: initializer.span(),
            swizzle,
            parenthesize: true,
        });
    }
}
/// Declaration initialized from a wider vector expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NarrowVectorInitializer {
    /// Source span covering the initializer expression.
    pub span: SourceSpan,
    /// Swizzle text to insert.
    pub swizzle: &'static str,
    /// Whether the expression must be grouped before applying the swizzle.
    pub parenthesize: bool,
}

impl NarrowVectorInitializer {
    /// Emits the narrowing swizzle insertion.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        if self.parenthesize {
            context
                .context()
                .fixups
                .push(Fixup::insert(self.span.start_point(), "(".to_owned()));
            context.context().fixups.push(Fixup::insert(
                self.span.end_point(),
                format!("){}", self.swizzle),
            ));
        } else {
            context
                .context()
                .fixups
                .push(Fixup::insert(self.span, self.swizzle.to_owned()));
        }
    }
}
/// Scalar declarations initialized from visible vector identifiers.
#[derive(Default)]
pub(super) struct ScalarVectorInitializers {
    /// Component-selection insertions in source order.
    pub items: Vec<ScalarVectorInitializer>,
}

impl ScalarVectorInitializers {
    /// Scans scalar declarations initialized from vector identifiers.
    pub(super) fn collect(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) {
        for fact in token_facts.declarations() {
            let Some(declaration) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            if declaration.ty() != "float" {
                continue;
            }
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            if initializer.start() != initializer.end() {
                continue;
            }
            let TypedToken::Identifier(name) = tokens[initializer.start()].kind() else {
                continue;
            };
            if !matches!(
                facts.lookup(name, initializer.start()),
                Some(BindingType::Vector(_))
            ) {
                continue;
            }
            let Ok(insertion) = SourceSpan::new(
                tokens[initializer.start()].span().end(),
                tokens[initializer.start()].span().end(),
            ) else {
                continue;
            };
            self.items.push(ScalarVectorInitializer { insertion });
        }
    }
}
/// Scalar declaration initialized by a vector identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarVectorInitializer {
    /// Source span immediately after the initializer identifier.
    pub insertion: SourceSpan,
}

impl ScalarVectorInitializer {
    /// Emits the component-selection insertion.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        context
            .context()
            .fixups
            .push(Fixup::insert(self.insertion, ".x".to_owned()));
    }
}

/// Scalar declarations initialized from calls that return vectors.
#[derive(Default)]
pub(super) struct ScalarVectorReturnInitializers {
    /// Component-selection insertions in source order.
    pub items: Vec<ScalarVectorReturnInitializer>,
}

impl ScalarVectorReturnInitializers {
    /// Scans scalar declarations initialized from vector-returning expressions.
    pub(super) fn collect(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        calls: &[FunctionCall],
        facts: &VectorTypeBindings<'_>,
    ) {
        for fact in token_facts.declarations() {
            let Some(declaration) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            if declaration.ty() != "float" {
                continue;
            }
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            if initializer.start() == initializer.end()
                && let TypedToken::Identifier(name) = tokens[initializer.start()].kind()
                && matches!(
                    facts.lookup(name, initializer.start()),
                    Some(BindingType::Vector(_))
                )
            {
                continue;
            }
            let call_width = match (
                tokens[initializer.start()].kind(),
                tokens.next_non_comment(initializer.start() + 1),
            ) {
                (TypedToken::Identifier(name), Some(open))
                    if tokens[open].kind().is_left_paren()
                        && tokens[initializer.end()].kind().is_right_paren() =>
                {
                    if let Some(function) = match name.as_str() {
                        "mix" | "lerp" => Some(CoercionFunction::Mix),
                        "smoothstep" => Some(CoercionFunction::Smoothstep),
                        "step" => Some(CoercionFunction::Step),
                        "pow" => Some(CoercionFunction::Pow),
                        "clamp" => Some(CoercionFunction::Clamp),
                        "min" => Some(CoercionFunction::Min),
                        "max" => Some(CoercionFunction::Max),
                        _ => None,
                    } {
                        calls
                            .iter()
                            .find(|call| {
                                call.name_index == initializer.start()
                                    && call.close_index == initializer.end()
                            })
                            .map(|call| &call.arguments)
                            .and_then(|arguments| {
                                arguments.coercion_width(tokens, token_facts, facts, function)
                            })
                    } else {
                        CallArgument::trim_from_bounds(
                            tokens,
                            initializer.start(),
                            initializer.end() + 1,
                        )
                        .and_then(|argument| {
                            VectorExpressionAnalyzer { facts, token_facts }
                                .call_return_width(tokens, argument)
                        })
                    }
                }
                _ => None,
            };
            let selection = if let Some(width) = call_width {
                ScalarVectorInitializerSelection {
                    span: initializer.span(),
                    width: Some(width),
                    parenthesize: false,
                }
            } else {
                ScalarVectorInitializerSelection {
                    span: initializer.span(),
                    width: initializer.vector_width(tokens, token_facts, facts),
                    parenthesize: true,
                }
            };
            if selection.width.is_none() {
                continue;
            }
            self.items.push(ScalarVectorReturnInitializer { selection });
        }
    }
}

/// Scalar initializer from a vector-returning expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarVectorReturnInitializer {
    /// Component-selection plan for the initializer expression.
    selection: ScalarVectorInitializerSelection,
}

impl ScalarVectorReturnInitializer {
    /// Emits the component-selection insertion.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        self.selection.emit(context);
    }
}

/// Component-selection plan for a vector-valued scalar initializer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScalarVectorInitializerSelection {
    /// Full initializer expression span.
    span: SourceSpan,
    /// Inferred vector width, when this initializer is vector-valued.
    width: Option<VectorWidth>,
    /// Whether the expression must be grouped before selecting `.x`.
    parenthesize: bool,
}

impl ScalarVectorInitializerSelection {
    /// Emits the `.x` selection for this initializer expression.
    fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        if self.width.is_none() {
            return;
        }
        if self.parenthesize {
            context
                .context()
                .fixups
                .push(Fixup::insert(self.span.start_point(), "(".to_owned()));
            context
                .context()
                .fixups
                .push(Fixup::insert(self.span.end_point(), ").x".to_owned()));
        } else {
            context
                .context()
                .fixups
                .push(Fixup::insert(self.span.end_point(), ".x".to_owned()));
        }
    }
}

impl DeclaratorInitializer {
    /// Returns the vector width produced by this expression.
    fn vector_width(
        self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        facts: &VectorTypeBindings<'_>,
    ) -> Option<VectorWidth> {
        if let Some(width) =
            Self::argument_width(tokens, token_facts, self.start(), self.end(), facts)
        {
            return Some(width);
        }
        let expression = token_facts.expression_covering(self.start()..self.end() + 1)?;
        let operators = expression.matching_top_level_operators(&[
            Arithmetic(ArithmeticOperator::Add),
            Arithmetic(ArithmeticOperator::Subtract),
            Arithmetic(ArithmeticOperator::Multiply),
            Arithmetic(ArithmeticOperator::Divide),
            Arithmetic(ArithmeticOperator::Remainder),
        ]);
        expression
            .operand_ranges_for(&operators)
            .into_iter()
            .filter_map(|range| {
                let end = range.end().checked_sub(1)?;
                Self::argument_width(tokens, token_facts, range.start(), end, facts)
            })
            .min()
    }

    /// Returns the vector width for one operand-like expression range.
    fn argument_width(
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        start: usize,
        end: usize,
        facts: &VectorTypeBindings<'_>,
    ) -> Option<VectorWidth> {
        let argument = CallArgument::trim_from_bounds(tokens, start, end + 1)?;
        VectorExpressionAnalyzer { facts, token_facts }.argument_vector_width(tokens, argument)
    }

    /// Returns whether a whole builtin call is repaired by call-argument
    /// coercion using the declaration context.
    fn is_context_coerced_call(
        self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
    ) -> bool {
        let Some(call) = token_facts.call_at_name(self.start()) else {
            return false;
        };
        if call.close_index() != self.end() {
            return false;
        }
        let Some(name) = tokens[self.start()].kind().source_text() else {
            return false;
        };
        matches!(
            name,
            "mix" | "lerp" | "smoothstep" | "step" | "pow" | "clamp" | "min" | "max"
        )
    }
}

/// Vector declarations whose scalar literal initializers need broadcasting.
#[derive(Default)]
pub(super) struct VectorScalarInitializers {
    /// Scalar initializer replacements in source order.
    pub items: Vec<VectorScalarInitializer>,
}

impl VectorScalarInitializers {
    /// Scans module tokens for vector declarations with scalar initializers.
    pub(super) fn collect(&mut self, tokens: TokenCursor<'_>, token_facts: &TypedTokenFacts) {
        for fact in token_facts.declarations() {
            let Some(width) = VectorWidth::classify_constructor(fact.ty().as_str()) else {
                continue;
            };
            let Some(range) = fact.initializer() else {
                continue;
            };
            let Some((start, end)) = tokens.non_comment_range(range.start(), range.end()) else {
                continue;
            };
            let (start, end) = match (start, end) {
                (start, end)
                    if start == end
                        && matches!(
                            tokens[start].kind(),
                            TypedToken::Literal(LiteralValue::Number(_))
                        ) =>
                {
                    (start, end)
                }
                (start, end)
                    if start + 1 == end
                        && matches!(
                            tokens[start].kind(),
                            TypedToken::Operator(Arithmetic(
                                ArithmeticOperator::Add | ArithmeticOperator::Subtract,
                            ))
                        )
                        && matches!(
                            tokens[end].kind(),
                            TypedToken::Literal(LiteralValue::Number(_))
                        ) =>
                {
                    (start, end)
                }
                _ => continue,
            };
            let Ok(span) = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end())
            else {
                continue;
            };
            self.items.push(VectorScalarInitializer { span, width });
        }
    }
}
/// Vector scalar initializer that needs constructor broadcasting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VectorScalarInitializer {
    /// Scalar literal span to replace.
    pub span: SourceSpan,
    /// Constructor width to emit.
    pub width: VectorWidth,
}

impl VectorScalarInitializer {
    /// Emits the scalar-to-vector constructor replacement.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
        let source = context.context().module.source();
        let literal = source.slice(self.span);
        context.context().fixups.push(Fixup::replace(
            self.span,
            format!("{}({literal})", self.width.constructor()),
        ));
    }
}
/// Integer literal converted to GLSL float literal spelling.
#[derive(Clone, Copy)]
pub(super) struct FloatLiteral<'src> {
    /// Original literal text.
    pub text: &'src str,
}

impl std::fmt::Display for FloatLiteral<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let trimmed = self.text.trim_end_matches(['u', 'U', 'l', 'L']);
        write!(formatter, "{trimmed}.0")
    }
}
