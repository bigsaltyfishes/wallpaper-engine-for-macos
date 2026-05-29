//! Borrowed token stream for Wallpaper Engine shader sources.

use logos::Logos;

use crate::{ShaderDiagnostic, ShaderError, ShaderResult, SourceSpan};

/// One token borrowed from a shader source buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token<'src> {
    /// Token kind and borrowed source text where useful.
    pub kind: TokenKind<'src>,
    /// Byte span in the original source.
    pub span: SourceSpan,
}

/// Borrowed shader token stream.
pub type TokenStream<'src> = Vec<Token<'src>>;

/// Construction methods for borrowed shader token streams.
pub trait TokenStreamExt<'src>: Sized {
    /// Lexes a shader source into borrowed tokens with byte spans.
    ///
    /// # Errors
    ///
    /// Returns a parse error when an input byte range cannot be represented as
    /// a valid [`SourceSpan`].
    fn lex(source: &'src str) -> ShaderResult<Self>;
}

impl<'src> TokenStreamExt<'src> for TokenStream<'src> {
    fn lex(source: &'src str) -> ShaderResult<Self> {
        let mut lexer = RawToken::lexer(source);
        let (lower, _) = lexer.size_hint();
        let mut tokens = Self::with_capacity(lower);
        let mut diagnostics = Vec::new();

        while let Some(raw_result) = lexer.next() {
            let range = lexer.span();
            match raw_result {
                Ok(raw) => tokens.push(Token {
                    kind: raw.into(),
                    span: SourceSpan::new(range.start, range.end)?,
                }),
                Err(()) => {
                    diagnostics.push(
                        ShaderDiagnostic::new("unrecognized shader token")
                            .with_span(SourceSpan::new(range.start, range.end)?),
                    );
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(tokens)
        } else {
            Err(ShaderError::Parse {
                diagnostics: diagnostics.into_boxed_slice(),
            })
        }
    }
}

/// Lightweight token categories used by the shader syntax model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind<'src> {
    /// Wallpaper Engine line annotation such as `[COMBO]` or JSON metadata.
    Annotation(&'src str),
    /// Ordinary line or block comment.
    Comment(&'src str),
    /// Preprocessor directive line.
    Directive(&'src str),
    /// Identifier or keyword text.
    Identifier(&'src str),
    /// Numeric literal text.
    Number(&'src str),
    /// String literal text.
    StringLiteral(&'src str),
    /// `{`.
    LeftBrace,
    /// `}`.
    RightBrace,
    /// `(`.
    LeftParen,
    /// `)`.
    RightParen,
    /// `;`.
    Semicolon,
    /// `,`.
    Comma,
    /// Any other single punctuation/operator character.
    Punctuation(char),
}

impl<'src> TokenKind<'src> {
    /// Returns identifier or keyword text when this token is an identifier.
    #[must_use]
    pub const fn identifier_text(self) -> Option<&'src str> {
        match self {
            Self::Identifier(text) => Some(text),
            _ => None,
        }
    }

    /// Returns whether this token is an ordinary source comment.
    #[must_use]
    pub const fn is_comment(self) -> bool {
        matches!(self, Self::Comment(_))
    }

    /// Returns whether this token is a declaration modifier rather than a
    /// declaration type or name.
    #[must_use]
    pub const fn is_declaration_modifier(self) -> bool {
        let Self::Identifier(text) = self else {
            return false;
        };

        matches!(
            text.as_bytes(),
            b"lowp"
                | b"mediump"
                | b"highp"
                | b"flat"
                | b"smooth"
                | b"noperspective"
                | b"centroid"
                | b"sample"
                | b"invariant"
                | b"const"
        )
    }
}

/// Raw logos token categories before conversion into public token kinds.
#[derive(Logos, Clone, Copy, Debug, Eq, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
enum RawToken<'src> {
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
    /// `(`.
    #[token("(")]
    LeftParen,
    /// `)`.
    #[token(")")]
    RightParen,
    /// `;`.
    #[token(";")]
    Semicolon,
    /// `,`.
    #[token(",")]
    Comma,

    /// Any remaining single punctuation or operator character.
    #[regex(r".", |lexer| lexer.slice().chars().next(), priority = 0)]
    Punctuation(char),
}

impl<'src> From<RawToken<'src>> for TokenKind<'src> {
    fn from(raw: RawToken<'src>) -> Self {
        match raw {
            RawToken::Annotation(text) => Self::Annotation(text),
            RawToken::Comment(text) => Self::Comment(text),
            RawToken::Directive(text) => Self::Directive(text),
            RawToken::Identifier(text) => Self::Identifier(text),
            RawToken::Number(text) => Self::Number(text),
            RawToken::StringLiteral(text) => Self::StringLiteral(text),
            RawToken::LeftBrace => Self::LeftBrace,
            RawToken::RightBrace => Self::RightBrace,
            RawToken::LeftParen => Self::LeftParen,
            RawToken::RightParen => Self::RightParen,
            RawToken::Semicolon => Self::Semicolon,
            RawToken::Comma => Self::Comma,
            RawToken::Punctuation(value) => Self::Punctuation(value),
        }
    }
}
