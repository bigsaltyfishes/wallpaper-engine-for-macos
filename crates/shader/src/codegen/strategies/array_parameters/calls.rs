use smol_str::SmolStr;

use super::{FunctionParameterList, ShaderModule, ShaderResult, SourceSpan, TypedToken};

/// Parsed arguments passed to all calls of one function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CallArguments {
    /// Function calls in source order.
    pub calls: Vec<FunctionCallArguments>,
}

impl CallArguments {
    /// Constructs compatible call arguments for one function signature.
    #[inline]
    #[allow(clippy::single_call_fn)]
    pub(super) fn new(
        module: &ShaderModule<'_>,
        name: &str,
        parameters: &FunctionParameterList,
    ) -> ShaderResult<Self> {
        let tokens = module.token_stream().cursor();
        let mut calls = Vec::new();
        for call in module.function_calls().filter(|call| call.name() == name) {
            let search = tokens;
            let is_definition_call = search
                .next_non_comment(call.name_index + 1)
                .and_then(|open| tokens.matching_right_paren(open))
                .and_then(|close| search.next_non_comment(close + 1))
                .is_some_and(|next| matches!(tokens[next].kind(), TypedToken::LeftBrace));
            if is_definition_call {
                continue;
            }
            let items = call
                .arguments
                .iter()
                .map(|argument| {
                    let identifier = argument
                        .is_single_token()
                        .then(|| {
                            tokens[argument.start()]
                                .kind()
                                .identifier_text()
                                .map(SmolStr::new)
                        })
                        .flatten();
                    FunctionCallArgument {
                        span: argument.span(),
                        identifier,
                    }
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                continue;
            }
            let span = SourceSpan::new(
                tokens[call.open_index].span().end(),
                tokens[call.close_index].span().start(),
            )?;
            let arguments = FunctionCallArguments { span, items };
            let matches_signature = arguments.items.len() == parameters.items.len()
                && arguments.items.iter().zip(parameters.items.iter()).all(
                    |(argument, parameter)| {
                        parameter.array.is_none()
                            || argument.identifier.as_ref().is_some_and(|name| {
                                module.has_top_level_array_declaration(name.as_str())
                            })
                    },
                );
            if !matches_signature {
                continue;
            }
            calls.push(arguments);
        }

        Ok(Self { calls })
    }
}

/// One parsed function call argument list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionCallArguments {
    /// Full source span between the call parentheses.
    pub span: SourceSpan,
    /// Argument segments in source order.
    pub items: Vec<FunctionCallArgument>,
}
/// One comma-delimited function call argument segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionCallArgument {
    /// Original source span for this argument segment.
    pub span: SourceSpan,
    /// Identifier text when the argument is exactly one identifier token.
    pub identifier: Option<SmolStr>,
}
/// One call-site replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SpecializedCall {
    /// Span between the call parentheses.
    pub span: SourceSpan,
    /// Replacement argument list.
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ShaderStageKind,
        syntax::{ShaderModule, SyntaxItem},
    };

    #[test]
    fn new_parses_array_helper_call_arguments() {
        let source = concat!(
            "uniform float values[4];\n",
            "void helper(float data[4], float scale) {}\n",
            "void main() { helper(values, 2.0); }\n",
        );
        let module = ShaderModule::parse(ShaderStageKind::Fragment, source).expect("module parses");
        let function = module
            .items()
            .iter()
            .find_map(|item| match item {
                SyntaxItem::Function(function) if function.name() == "helper" => Some(function),
                _ => None,
            })
            .expect("helper exists");
        let parameters = FunctionParameterList::parse(function);

        let calls = CallArguments::new(&module, "helper", &parameters).expect("calls collect");

        assert_eq!(calls.calls.len(), 1);
        assert_eq!(calls.calls[0].items.len(), 2);
        assert_eq!(
            calls.calls[0].items[0].identifier.as_deref(),
            Some("values")
        );
        assert_eq!(calls.calls[0].items[1].identifier, None);
    }
}
