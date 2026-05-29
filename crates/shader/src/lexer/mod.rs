//! Primitive Logos lexer for Wallpaper Engine shader sources.

use logos::Logos;
use smol_str::SmolStr;

use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, SourceSpan,
    tokenizer::{LiteralValue, OperatorType, Token, TokenStream, TypedToken},
};

/// Primitive token producer at the Logos boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lexer;

impl Lexer {
    /// Tokenizes a shader source into lifetime-free typed tokens with byte
    /// spans.
    ///
    /// # Errors
    ///
    /// Returns a parse error when an input byte range cannot be represented as
    /// a valid [`SourceSpan`].
    pub fn tokenize(source: &str) -> ShaderResult<TokenStream> {
        let mut lexer = RawToken::lexer(source);
        let (lower, _) = lexer.size_hint();
        let mut tokens = Vec::with_capacity(lower);
        let mut diagnostics = Vec::new();

        while let Some(raw_result) = lexer.next() {
            let range = lexer.span();
            match raw_result {
                Ok(raw) => {
                    let span = SourceSpan::new(range.start, range.end)?;
                    match raw.into_typed() {
                        RawTypedToken::Token(kind) => tokens.push(Token::new(kind, span)),
                        RawTypedToken::Discard => {}
                    }
                }
                Err(()) => {
                    diagnostics.push(
                        ShaderDiagnostic::new("unrecognized shader token")
                            .with_span(SourceSpan::new(range.start, range.end)?),
                    );
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(TokenStream::new(tokens))
        } else {
            Err(ShaderError::Parse {
                diagnostics: diagnostics.into_boxed_slice(),
            })
        }
    }
}

/// Raw Logos token categories before conversion into typed tokens.
#[derive(Logos, Clone, Copy, Debug, Eq, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
pub enum RawToken<'src> {
    /// Wallpaper Engine annotation line captured from a comment token.
    #[regex(
        r"//[ \t]*(\[[A-Z0-9_]+\]|\{)[^\n\r]*",
        |lex| lex.slice(),
        priority = 3,
        allow_greedy = true
    )]
    Annotation(&'src str),

    /// Ordinary line or block comment.
    #[regex(
        r"//[^\n\r]*",
        |lex| lex.slice(),
        priority = 2,
        allow_greedy = true
    )]
    #[regex(r"/\*([^*]|\*+[^*/])*\*+/", |lex| lex.slice())]
    Comment(&'src str),

    /// Preprocessor directive, including backslash-continued lines.
    #[regex(
        r"#[^\n\r]*(\\\r?\n[^\n\r]*)*",
        |lex| lex.slice(),
        priority = 3,
        allow_greedy = true
    )]
    Directive(&'src str),

    /// Double-quoted string literal.
    #[regex(r#""([^"\\\n\r]|\\.)*""#, |lex| lex.slice())]
    StringLiteral(&'src str),

    /// Decimal numeric literal.
    #[regex(
        r"([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?[uUlLfF]*",
        |lex| lex.slice()
    )]
    Number(&'src str),

    /// Identifier or keyword.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice())]
    Identifier(&'src str),

    /// `{`.
    #[token("{")]
    LeftBrace,
    /// `}`.
    #[token("}")]
    RightBrace,
    /// `;`.
    #[token(";")]
    Semicolon,
    /// `,`.
    #[token(",")]
    Comma,

    /// GLSL operator lexeme. Longer operators are matched before their
    /// prefixes by Logos' longest-match rule.
    #[regex(r"(\+\+|--|<<=|>>=|\+=|-=|\*=|/=|%=|&=|\^=|\|=|<<|>>|<=|>=|==|!=|&&|\|\||\^\^|[+\-*/%=!<>&|^~?:.]|\(|\)|\[|\])", |lexer| lexer.slice())]
    Operator(&'src str),

    /// Any remaining single punctuation character.
    #[regex(r".", |lexer| lexer.slice().chars().next(), priority = 0)]
    RawGlyph(char),
}

impl RawToken<'_> {
    /// Converts a raw lexeme into a lifetime-free typed token when the lexeme
    /// is retained after lexing.
    fn into_typed(self) -> RawTypedToken {
        match self {
            Self::Annotation(text) => {
                RawTypedToken::Token(TypedToken::Annotation(SmolStr::new(text)))
            }
            Self::Comment(_) => RawTypedToken::Discard,
            Self::Directive(text) => {
                RawTypedToken::Token(TypedToken::Directive(SmolStr::new(text)))
            }
            Self::Identifier(text) => RawTypedToken::Token(TypedToken::from_identifier_text(text)),
            Self::Number(text) => RawTypedToken::Token(TypedToken::Literal(LiteralValue::Number(
                SmolStr::new(text),
            ))),
            Self::StringLiteral(text) => {
                RawTypedToken::Token(TypedToken::StringLiteral(SmolStr::new(text)))
            }
            Self::LeftBrace => RawTypedToken::Token(TypedToken::LeftBrace),
            Self::RightBrace => RawTypedToken::Token(TypedToken::RightBrace),
            Self::Semicolon => RawTypedToken::Token(TypedToken::Semicolon),
            Self::Comma => RawTypedToken::Token(TypedToken::Comma),
            Self::Operator(text) => RawTypedToken::Token(OperatorType::parse(text).map_or_else(
                || TypedToken::Other(SmolStr::new(text)),
                TypedToken::Operator,
            )),
            Self::RawGlyph(glyph) => {
                let mut encoded = [0; 4];
                RawTypedToken::Token(TypedToken::Other(SmolStr::new(
                    glyph.encode_utf8(&mut encoded),
                )))
            }
        }
    }
}

/// Result of converting a raw Logos token into tokenizer-stage output.
enum RawTypedToken {
    /// A typed token that should be retained.
    Token(TypedToken),
    /// Trivia or comments that should not enter the token stream.
    Discard,
}
