use shader::{
    ShaderStageKind,
    syntax::ShaderModule,
    tokenizer::{
        AccessOperator, ArithmeticOperator, AssignmentOperator, BitwiseOperator,
        ConditionalOperator, EqualityOperator, GroupingOperator, IncrementOperator, KeywordType,
        LiteralValue, LogicalOperator,
        OperatorType::{
            Access, Arithmetic, Assignment, Bitwise, Conditional, Equality, Grouping, Increment,
            Logical, Relational, Subscript,
        },
        PrimitiveType, RelationalOperator, SubscriptOperator, TokenStream, TypedToken,
        TypedTokenFacts,
    },
};

#[test]
fn tokenizer_owns_identifier_text_without_source_lifetime() {
    let source = String::from("uniform float customName;");
    let stream = TokenStream::lex(&source).expect("source tokenizes");
    drop(source);

    let identifier = stream
        .iter()
        .find_map(|token| match token.kind() {
            TypedToken::Identifier(name) if name == "customName" => Some(name.clone()),
            _ => None,
        })
        .expect("identifier is owned by token stream");

    assert_eq!(identifier.as_str(), "customName");
}

#[test]
fn tokenizer_reports_comma_declarator_initializer_ranges() {
    let stream = TokenStream::lex("vec3 a = color, b = vec3(1.0);").expect("source tokenizes");
    let facts = stream.facts();
    let declarations = facts.declarations();

    assert_eq!(declarations.len(), 2);
    assert_eq!(declarations[0].statement().start(), 0);
    assert_eq!(declarations[0].statement().end(), 12);
    assert_eq!(declarations[0].declarator().start(), 1);
    assert_eq!(declarations[0].declarator().end(), 5);
    assert_eq!(declarations[0].name().as_str(), "a");
    assert_eq!(
        declarations[0]
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((3, 4))
    );
    assert_eq!(declarations[1].statement().start(), 0);
    assert_eq!(declarations[1].statement().end(), 12);
    assert_eq!(declarations[1].declarator().start(), 5);
    assert_eq!(declarations[1].declarator().end(), 12);
    assert_eq!(declarations[1].name().as_str(), "b");
    assert_eq!(
        declarations[1]
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((7, 11))
    );
    assert_eq!(
        facts
            .declarations_at_statement_start(0)
            .iter()
            .map(|declaration| declaration.name().as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(
        facts
            .declaration_at_name(5)
            .map(|declaration| declaration.name().as_str()),
        Some("b")
    );
}

#[test]
fn tokenizer_finds_declarations_by_name_index_after_sorted_collection() {
    let stream = TokenStream::lex("void main(){ float z; for (int i = 0, j = 1; i < 4; ++i) {} }")
        .expect("source tokenizes");
    let facts = stream.facts();

    let name_indexes = facts
        .declarations()
        .iter()
        .map(|declaration| (declaration.name().to_string(), declaration.name_index()))
        .collect::<Vec<_>>();

    assert_eq!(
        name_indexes,
        [("z".into(), 6), ("i".into(), 11), ("j".into(), 15)]
    );
    for (name, name_index) in name_indexes {
        assert_eq!(
            facts
                .declaration_at_name(name_index)
                .map(|declaration| declaration.name().as_str()),
            Some(name.as_str())
        );
    }
    assert!(facts.declaration_at_name(12).is_none());
}

#[test]
fn tokenizer_reports_declarations_with_modifiers_and_array_suffixes() {
    let stream = TokenStream::lex("const highp vec4 values[2] = vec4[2](a, b), other[3];")
        .expect("source tokenizes");
    let facts: TypedTokenFacts = stream.facts();
    let declarations = facts.declarations();

    assert_eq!(declarations.len(), 2);
    assert_eq!(declarations[0].ty().as_str(), "vec4");
    assert_eq!(declarations[0].name().as_str(), "values");
    assert!(declarations[0].initializer().is_some());
    assert_eq!(declarations[1].name().as_str(), "other");
    assert!(declarations[1].initializer().is_none());
}

#[test]
fn tokenizer_reports_for_loop_sections_without_strategy_parsing() {
    let stream = TokenStream::lex("for (int i = 0; i < count; i += step) { value += i; }")
        .expect("source tokenizes");
    let facts = stream.facts();
    let for_loop = facts.for_loops().first().expect("for loop fact exists");

    assert_eq!(for_loop.range().start(), 0);
    assert_eq!(for_loop.range().end(), stream.len());
    assert_eq!(
        for_loop
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((2, 6))
    );
    assert_eq!(
        for_loop
            .condition()
            .map(|range| (range.start(), range.end())),
        Some((7, 10))
    );
    assert_eq!(
        for_loop.step().map(|range| (range.start(), range.end())),
        Some((11, 14))
    );
    assert_eq!(for_loop.body().start(), 15);
    assert_eq!(for_loop.body().end(), stream.len());
}

#[test]
fn tokenizer_reports_control_flow_condition_ranges() {
    let stream =
        TokenStream::lex("if (brightness) { color = vec3(1.0); }").expect("source tokenizes");
    let facts = stream.facts();
    let condition = facts.conditions().first().expect("condition fact exists");

    assert_eq!(condition.range().start(), 2);
    assert_eq!(condition.range().end(), 3);
}

#[test]
fn tokenizer_orders_nested_control_flow_facts_by_source_start() {
    let stream =
        TokenStream::lex("for (int i = 0; i < count; ++i) { if (enabled) { color += vec3(i); } }")
            .expect("source tokenizes");
    let facts = stream.facts();

    let statement_starts = facts
        .statements()
        .iter()
        .map(|statement| statement.range().start())
        .collect::<Vec<_>>();
    assert!(
        statement_starts
            .windows(2)
            .all(|window| window[0] <= window[1]),
        "statement starts are not source ordered: {statement_starts:?}",
    );

    let for_index = facts
        .statements()
        .iter()
        .position(|statement| {
            matches!(
                statement.kind(),
                shader::tokenizer::StatementKind::For { .. }
            )
        })
        .expect("for statement fact exists");
    let if_index = facts
        .statements()
        .iter()
        .position(|statement| {
            matches!(
                statement.kind(),
                shader::tokenizer::StatementKind::If { .. }
            )
        })
        .expect("if statement fact exists");
    assert!(for_index < if_index);

    let for_loop_starts = facts
        .for_loops()
        .iter()
        .map(|statement| statement.range().start())
        .collect::<Vec<_>>();
    assert!(
        for_loop_starts
            .windows(2)
            .all(|window| window[0] <= window[1]),
        "for loop starts are not source ordered: {for_loop_starts:?}",
    );

    let condition_starts = facts
        .conditions()
        .iter()
        .map(|condition| condition.range().start())
        .collect::<Vec<_>>();
    assert!(
        condition_starts
            .windows(2)
            .all(|window| window[0] <= window[1]),
        "condition starts are not source ordered: {condition_starts:?}",
    );
    assert_eq!(condition_starts.len(), 2);
    assert!(condition_starts[0] < condition_starts[1]);
}

#[test]
fn tokenizer_reports_while_do_while_and_else_statement_ranges() {
    let stream = TokenStream::lex(concat!(
        "while (count) value += 1;",
        "do { value -= 1; } while (value);",
        "if (enabled) { color = vec3(1.0); } else { color = vec3(0.0); }",
    ))
    .expect("source tokenizes");
    let facts = stream.facts();

    assert_eq!(facts.conditions().len(), 3);
    assert!(facts.statements().iter().any(|statement| {
        matches!(
            statement.kind(),
            shader::tokenizer::StatementKind::While { condition, body }
                if condition.start() == 2 && condition.end() == 3 && body.start() == 4
        )
    }));
    assert!(facts.statements().iter().any(|statement| {
        matches!(
            statement.kind(),
            shader::tokenizer::StatementKind::DoWhile { body, condition }
                if body.start() == 9 && condition.start() == 17 && condition.end() == 18
        )
    }));
    assert!(facts.statements().iter().any(|statement| {
        matches!(
            statement.kind(),
            shader::tokenizer::StatementKind::If {
                condition,
                body,
                else_body: Some(else_body),
            } if condition.start() == 22
                && body.start() == 24
                && else_body.start() > body.end()
        )
    }));
}

#[test]
fn tokenizer_declaration_facts_ignore_calls_constructors_and_control_headers() {
    let stream = TokenStream::lex(concat!(
        "void main() {",
        "vec3(1.0);",
        "make_vec3(value);",
        "if (float(value)) { value = 1.0; }",
        "for (int i = 0; i < 4; ++i) { value += float(i); }",
        "vec3 color = vec3(1.0);",
        "}",
    ))
    .expect("source tokenizes");
    let facts = stream.facts();
    let names = facts
        .declarations()
        .iter()
        .map(|declaration| declaration.name().as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, ["i", "color"]);
    assert!(facts.declaration_at_name(3).is_none());
    assert!(facts.declarations_at_statement_start(4).is_empty());
}

#[test]
fn tokenizer_declaration_facts_handle_nested_initializer_delimiters() {
    let stream = TokenStream::lex(concat!(
        "float values[2] = float[2](1.0, 2.0), ",
        "selected = table[int(indices[0])], ",
        "aggregate = Payload(float[2](1.0, 2.0), Payload{3.0, 4.0});",
    ))
    .expect("source tokenizes");
    let facts = stream.facts();
    let declarations = facts.declarations();

    assert_eq!(
        declarations
            .iter()
            .map(|declaration| declaration.name().as_str())
            .collect::<Vec<_>>(),
        ["values", "selected", "aggregate"]
    );
    assert_eq!(
        declarations[0]
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((6, 15))
    );
    assert_eq!(
        declarations[1]
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((18, 28))
    );
    assert_eq!(
        declarations[2]
            .initializer()
            .map(|range| (range.start(), range.end())),
        Some((31, 50))
    );
}

#[test]
fn tokenizer_reports_function_call_arguments_once() {
    let stream = TokenStream::lex("color = max(vec3(0.0), sample.rgb);").expect("source tokenizes");
    let facts = stream.facts();
    let call = facts
        .calls()
        .iter()
        .find(|call| call.name() == "max")
        .expect("max call");

    assert_eq!(call.arguments().len(), 2);
}

#[test]
fn tokenizer_reports_top_level_binary_segments() {
    let stream = TokenStream::lex("a + b * (c + d)").expect("source tokenizes");
    let facts = stream.facts();
    let expression = facts
        .expression_covering(0..stream.len())
        .expect("expression fact");
    let operators = expression
        .top_level_operators()
        .iter()
        .map(|operator| (operator.index(), operator.operator()))
        .collect::<Vec<_>>();

    assert_eq!(operators, [(1, Arithmetic(ArithmeticOperator::Add))]);
}

#[test]
fn tokenizer_reports_binary_expression_operand_ranges() {
    let stream = TokenStream::lex("uv + normal * scale").expect("source tokenizes");
    let facts = stream.facts();
    let expression = facts
        .expression_covering(0..stream.len())
        .expect("expression fact");
    let operands = facts
        .binary_expression_operand_ranges(
            stream.cursor(),
            expression.range(),
            &[
                Arithmetic(ArithmeticOperator::Add),
                Arithmetic(ArithmeticOperator::Multiply),
            ],
        )
        .into_iter()
        .map(|range| {
            stream.cursor()[range.start()..range.end()]
                .iter()
                .filter_map(|token| token.kind().source_text())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(operands, ["uv", "normal", "normal*scale", "scale"]);
}

#[test]
fn tokenizer_reports_float_modulo_operand_ranges() {
    let stream = TokenStream::lex("value = time % speed;").expect("source tokenizes");
    let facts = stream.facts();
    let expression = facts
        .expression_covering(2..stream.len() - 1)
        .expect("rhs expression");
    let operators = expression
        .top_level_operators()
        .iter()
        .map(|operator| (operator.index(), operator.operator()))
        .collect::<Vec<_>>();

    assert_eq!(operators, [(3, Arithmetic(ArithmeticOperator::Remainder))]);
}

#[test]
fn tokenizer_reports_modulo_lowering_segments() {
    let stream = TokenStream::lex("value = a % b + c * (d % e), f ? g % h : i[j % k] <= l % m;")
        .expect("source tokenizes");
    let facts = stream.facts();
    let expression = facts
        .expression_covering(0..stream.len() - 1)
        .expect("statement expression");
    let segments = expression
        .modulo_lowering_segments()
        .iter()
        .map(|range| {
            stream.cursor()[range.start()..range.end()]
                .iter()
                .filter_map(|token| token.kind().source_text())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        segments,
        ["value", "a%b", "c*(d%e)", "f", "g%h", "i[j%k]", "l%m"]
    );
}

#[test]
fn tokenizer_reports_function_parameter_facts() {
    let module = ShaderModule::parse(
        ShaderStageKind::Fragment,
        "float mod(float value, float divisor) { return value; }",
    )
    .expect("module parses");
    let function = module
        .functions()
        .iter()
        .find(|function| function.name() == "mod")
        .expect("mod function");

    assert_eq!(function.parameters().len(), 2);
    assert_eq!(function.parameters()[0].name_text(), Some("value"));
    assert_eq!(function.parameters()[0].ty().as_str(), "float");
    assert_eq!(function.parameters()[1].name_text(), Some("divisor"));
    assert_eq!(function.parameters()[1].ty().as_str(), "float");
}

#[test]
fn tokenizer_reports_function_signature_facts_for_definitions_and_prototypes() {
    let stream = TokenStream::lex(
        "float helper(float value, vec2 offset); vec3 compose(vec3 color) { return color; }",
    )
    .expect("source tokenizes");
    let facts = stream.facts();
    let signatures = facts.function_signatures();

    assert_eq!(signatures.len(), 2);
    assert_eq!(signatures[0].name().as_str(), "helper");
    assert_eq!(signatures[0].return_type().as_str(), "float");
    assert_eq!(
        signatures[0]
            .parameters()
            .iter()
            .map(|parameter| parameter.ty().as_str())
            .collect::<Vec<_>>(),
        ["float", "vec2"]
    );
    assert_eq!(signatures[1].name().as_str(), "compose");
    assert_eq!(signatures[1].return_type().as_str(), "vec3");
    assert_eq!(
        signatures[1]
            .parameters()
            .iter()
            .map(|parameter| parameter.ty().as_str())
            .collect::<Vec<_>>(),
        ["vec3"]
    );
}

#[test]
fn tokenizer_function_signature_facts_ignore_call_expressions() {
    let stream = TokenStream::lex("void main() { value = helper(value); return other(); }")
        .expect("source tokenizes");
    let facts = stream.facts();
    let signatures = facts.function_signatures();

    assert_eq!(signatures.len(), 1);
    assert_eq!(signatures[0].name().as_str(), "main");
}

#[test]
fn tokenizer_reports_unnamed_function_definition_parameter_facts() {
    let module = ShaderModule::parse(
        ShaderStageKind::Fragment,
        "float passthrough(float) { return 0.0; }",
    )
    .expect("module parses");
    let function = module
        .functions()
        .iter()
        .find(|function| function.name() == "passthrough")
        .expect("passthrough function");

    assert_eq!(function.parameters().len(), 1);
    assert_eq!(function.parameters()[0].name(), None);
    assert_eq!(function.parameters()[0].ty().as_str(), "float");
}

#[test]
fn tokenizer_reports_unnamed_user_type_function_parameter_facts() {
    let module = ShaderModule::parse(
        ShaderStageKind::Fragment,
        "struct Payload { float value; }; float passthrough(Payload) { return 0.0; }",
    )
    .expect("module parses");
    let function = module
        .functions()
        .iter()
        .find(|function| function.name() == "passthrough")
        .expect("passthrough function");

    assert_eq!(function.parameters().len(), 1);
    assert_eq!(function.parameters()[0].name(), None);
    assert_eq!(function.parameters()[0].ty().as_str(), "Payload");
}

#[test]
fn tokenizer_reports_array_function_parameter_name_before_identifier_bound() {
    let module = ShaderModule::parse(
        ShaderStageKind::Fragment,
        "const int COUNT = 4; float helper(float values[COUNT]) { return values[0]; }",
    )
    .expect("module parses");
    let function = module
        .functions()
        .iter()
        .find(|function| function.name() == "helper")
        .expect("helper function");

    assert_eq!(function.parameters().len(), 1);
    assert_eq!(function.parameters()[0].name_text(), Some("values"));
    assert_eq!(function.parameters()[0].ty().as_str(), "float");
}

#[test]
fn tokenizer_classifies_primitive_vector_types() {
    let stream = TokenStream::lex("vec3 color = vec3(1.0);").expect("source tokenizes");
    assert!(
        stream
            .iter()
            .any(|token| matches!(token.kind(), TypedToken::TypeMark(PrimitiveType::Vec(3))))
    );
}

#[test]
fn tokenizer_classifies_keywords_literals_and_punctuation() {
    let stream = TokenStream::lex("uniform float value = 1.0;").expect("source tokenizes");
    let kinds: Vec<_> = stream.iter().map(|token| token.kind().clone()).collect();

    assert_eq!(
        kinds,
        vec![
            TypedToken::Keyword(KeywordType::Uniform),
            TypedToken::TypeMark(PrimitiveType::Float),
            TypedToken::Identifier("value".into()),
            TypedToken::Operator(Assignment(AssignmentOperator::Assign)),
            TypedToken::Literal(LiteralValue::Number("1.0".into())),
            TypedToken::Semicolon,
        ]
    );
}

#[test]
fn tokenizer_preserves_bitwise_not_as_typed_operator() {
    let stream = TokenStream::lex("int x = ~mask;").expect("source tokenizes");
    let kinds: Vec<_> = stream.iter().map(|token| token.kind().clone()).collect();

    assert_eq!(
        kinds,
        vec![
            TypedToken::TypeMark(PrimitiveType::Int),
            TypedToken::Identifier("x".into()),
            TypedToken::Operator(Assignment(AssignmentOperator::Assign)),
            TypedToken::Operator(Bitwise(BitwiseOperator::Not)),
            TypedToken::Identifier("mask".into()),
            TypedToken::Semicolon,
        ]
    );
}

#[test]
fn tokenizer_preserves_unrecognized_raw_punctuation_as_other() {
    let stream = TokenStream::lex("int x = `mask;").expect("unknown punctuation is retained");
    let kinds: Vec<_> = stream.iter().map(|token| token.kind().clone()).collect();

    assert_eq!(
        kinds,
        vec![
            TypedToken::TypeMark(PrimitiveType::Int),
            TypedToken::Identifier("x".into()),
            TypedToken::Operator(Assignment(AssignmentOperator::Assign)),
            TypedToken::Other("`".into()),
            TypedToken::Identifier("mask".into()),
            TypedToken::Semicolon,
        ]
    );
}

#[test]
fn tokenizer_discards_comments_but_preserves_annotations_and_directives() {
    let source = concat!(
        "// ordinary comment\n",
        "// [COMBO] {\"combo\":\"ENABLE\",\"default\":0}\n",
        "#define ENABLE 1\n",
        "void main() {}\n",
    );

    let stream = TokenStream::lex(source).expect("source tokenizes");

    assert!(stream.iter().all(|token| !matches!(
        token.kind(),
        TypedToken::Operator(Arithmetic(ArithmeticOperator::Divide))
    )));
    assert!(stream.iter().any(|token| matches!(
        token.kind(),
        TypedToken::Annotation(text) if text == "// [COMBO] {\"combo\":\"ENABLE\",\"default\":0}"
    )));
    assert!(stream.iter().any(|token| matches!(
        token.kind(),
        TypedToken::Directive(text) if text == "#define ENABLE 1"
    )));
}

#[test]
fn tokenizer_classifies_glsl_450_operators_by_domain() {
    let source = concat!(
        "a[i] = float(1.0); a.member++; --b; +c - d * e / f % g; ",
        "h << i >> j; k < l > m <= n >= o == p != q; ",
        "~r & s ^ t | u && v || !w ^^ q; x ? y : z; ",
        "aa += bb; cc -= dd; ee *= ff; gg /= hh; ii %= jj; ",
        "kk <<= ll; mm >>= nn; oo &= pp; qq ^= rr; ss |= tt;"
    );
    let stream = TokenStream::lex(source).expect("source tokenizes");
    let operators: Vec<_> = stream
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Operator(operator) => Some((operator.text(), *operator)),
            _ => None,
        })
        .collect();

    assert!(operators.contains(&("(", Grouping(GroupingOperator::LeftParen))));
    assert!(operators.contains(&(")", Grouping(GroupingOperator::RightParen))));
    assert!(operators.contains(&("[", Subscript(SubscriptOperator::LeftSquare))));
    assert!(operators.contains(&("]", Subscript(SubscriptOperator::RightSquare))));
    assert!(operators.contains(&(".", Access(AccessOperator::Member))));
    assert!(operators.contains(&("++", Increment(IncrementOperator::Increment))));
    assert!(operators.contains(&("--", Increment(IncrementOperator::Decrement))));
    assert!(operators.contains(&("+", Arithmetic(ArithmeticOperator::Add))));
    assert!(operators.contains(&("-", Arithmetic(ArithmeticOperator::Subtract))));
    assert!(operators.contains(&("*", Arithmetic(ArithmeticOperator::Multiply))));
    assert!(operators.contains(&("/", Arithmetic(ArithmeticOperator::Divide))));
    assert!(operators.contains(&("%", Arithmetic(ArithmeticOperator::Remainder))));
    assert!(operators.contains(&("<<", Bitwise(BitwiseOperator::ShiftLeft))));
    assert!(operators.contains(&(">>", Bitwise(BitwiseOperator::ShiftRight))));
    assert!(operators.contains(&("<", Relational(RelationalOperator::Less))));
    assert!(operators.contains(&(">", Relational(RelationalOperator::Greater))));
    assert!(operators.contains(&("<=", Relational(RelationalOperator::LessEqual))));
    assert!(operators.contains(&(">=", Relational(RelationalOperator::GreaterEqual))));
    assert!(operators.contains(&("==", Equality(EqualityOperator::Equal))));
    assert!(operators.contains(&("!=", Equality(EqualityOperator::NotEqual))));
    assert!(operators.contains(&("~", Bitwise(BitwiseOperator::Not))));
    assert!(operators.contains(&("&", Bitwise(BitwiseOperator::And))));
    assert!(operators.contains(&("^", Bitwise(BitwiseOperator::Xor))));
    assert!(operators.contains(&("|", Bitwise(BitwiseOperator::Or))));
    assert!(operators.contains(&("&&", Logical(LogicalOperator::And))));
    assert!(operators.contains(&("||", Logical(LogicalOperator::Or))));
    assert!(operators.contains(&("^^", Logical(LogicalOperator::Xor))));
    assert!(operators.contains(&("!", Logical(LogicalOperator::Not))));
    assert!(operators.contains(&("?", Conditional(ConditionalOperator::Question))));
    assert!(operators.contains(&(":", Conditional(ConditionalOperator::Colon))));
    assert!(operators.contains(&("=", Assignment(AssignmentOperator::Assign))));
    assert!(operators.contains(&("+=", Assignment(AssignmentOperator::AddAssign))));
    assert!(operators.contains(&("-=", Assignment(AssignmentOperator::SubtractAssign))));
    assert!(operators.contains(&("*=", Assignment(AssignmentOperator::MultiplyAssign))));
    assert!(operators.contains(&("/=", Assignment(AssignmentOperator::DivideAssign))));
    assert!(operators.contains(&("%=", Assignment(AssignmentOperator::RemainderAssign))));
    assert!(operators.contains(&("<<=", Assignment(AssignmentOperator::ShiftLeftAssign))));
    assert!(operators.contains(&(">>=", Assignment(AssignmentOperator::ShiftRightAssign))));
    assert!(operators.contains(&("&=", Assignment(AssignmentOperator::AndAssign))));
    assert!(operators.contains(&("^=", Assignment(AssignmentOperator::XorAssign))));
    assert!(operators.contains(&("|=", Assignment(AssignmentOperator::OrAssign))));
}

#[test]
fn tokenizer_preserves_multi_character_operator_spans() {
    let source = "a<=b == c && d ^^ e <<= f++";
    let stream = TokenStream::lex(source).expect("source tokenizes");
    let spans: Vec<_> = stream
        .iter()
        .filter_map(|token| match token.kind() {
            TypedToken::Operator(operator) => Some((operator.text(), token.span())),
            _ => None,
        })
        .collect();

    assert_eq!(spans[0].0, "<=");
    assert_eq!(spans[0].1.start(), 1);
    assert_eq!(spans[0].1.end(), 3);
    assert_eq!(spans[1].0, "==");
    assert_eq!(spans[1].1.start(), 5);
    assert_eq!(spans[1].1.end(), 7);
    assert_eq!(spans[2].0, "&&");
    assert_eq!(spans[2].1.start(), 10);
    assert_eq!(spans[2].1.end(), 12);
    assert_eq!(spans[3].0, "^^");
    assert_eq!(spans[3].1.start(), 15);
    assert_eq!(spans[3].1.end(), 17);
    assert_eq!(spans[4].0, "<<=");
    assert_eq!(spans[4].1.start(), 20);
    assert_eq!(spans[4].1.end(), 23);
    assert_eq!(spans[5].0, "++");
    assert_eq!(spans[5].1.start(), 25);
    assert_eq!(spans[5].1.end(), 27);
}
