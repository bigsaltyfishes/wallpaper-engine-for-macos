//! Preprocessor directive syntax records.

use super::{ShaderModule, ShaderSourceText, source::SpannedSyntax};
use crate::{IncludePath, SourceSpan};

/// Preprocessor directive line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreprocessorDirective<'src> {
    /// Parsed directive semantics retained from syntax parsing.
    kind: DirectiveKind<'src>,
    /// Source span covering the directive line.
    span: SourceSpan,
}

impl PreprocessorDirective<'static> {
    /// Creates a directive record.
    #[must_use]
    pub fn new(span: SourceSpan) -> Self {
        Self::from_token_text("", span)
    }
}

impl<'src> PreprocessorDirective<'src> {
    /// Creates a directive record from token text and source span.
    #[must_use]
    pub fn from_token_text(text: &'src str, span: SourceSpan) -> Self {
        let trimmed = text
            .trim()
            .strip_prefix('#')
            .map_or(text.trim(), str::trim_start);
        let (keyword, raw_body) = trimmed
            .split_once(char::is_whitespace)
            .map_or((trimmed, ""), |(name, rest)| (name, rest.trim_start()));
        let name = DirectiveName {
            raw: trimmed,
            source: keyword,
        };
        let body = DirectiveBody::new(DirectiveBody::new(raw_body).without_trailing_comment());
        let kind = match name.as_str() {
            "include" => DirectiveKind::Include(IncludeDirective { name, body }),
            "define" => DirectiveKind::Define(DefineDirective { name, body }),
            "if" | "ifdef" | "ifndef" | "elif" | "else" | "endif" => {
                DirectiveKind::Conditional(ConditionalDirective { name, body })
            }
            _ => DirectiveKind::Other { name, body },
        };

        Self { kind, span }
    }

    /// Returns the parsed directive semantics.
    #[must_use]
    pub const fn kind(&self) -> DirectiveKind<'src> {
        self.kind
    }

    /// Returns directive text without the leading `#`.
    #[must_use]
    pub const fn raw_text(&self) -> &'src str {
        self.kind.raw()
    }

    /// Returns the directive keyword.
    #[must_use]
    pub const fn name_text(&self) -> &'src str {
        self.kind.name().as_str()
    }

    /// Returns directive body text.
    #[must_use]
    pub const fn body_text(&self) -> &'src str {
        self.kind.body().as_str()
    }

    /// Returns the include path for an `#include` directive when present.
    ///
    /// # Errors
    ///
    /// Returns an error when this is an include directive with an invalid path.
    pub fn include_path(&self) -> Result<Option<IncludePath>, &'static str> {
        let Some(include) = self.kind.include() else {
            return Ok(None);
        };
        include.include_path().map(Some)
    }

    /// Returns parsed define signature and replacement facts when this is a
    /// `#define` directive.
    ///
    /// # Errors
    ///
    /// Returns an error when this is a define directive without a macro
    /// signature.
    pub fn define_parts(&self) -> Result<Option<DefineDirectiveParts<'src>>, &'static str> {
        let Some(define) = self.kind.define() else {
            return Ok(None);
        };
        let body = define.body().as_str();
        if body.is_empty() {
            return Err("#define expects a macro name");
        }

        let (signature, value) =
            body.split_once(char::is_whitespace)
                .map_or((body, "1"), |(signature, value)| {
                    let value = value.trim();
                    (signature, if value.is_empty() { "1" } else { value })
                });

        Ok(Some(DefineDirectiveParts {
            signature: DirectiveBody::new(signature),
            value: DirectiveBody::new(value),
        }))
    }

    /// Returns the typed conditional directive when this is a conditional.
    #[must_use]
    pub const fn conditional(&self) -> Option<ConditionalDirective<'src>> {
        self.kind.conditional()
    }

    /// Returns whether this is an `#include` directive.
    #[must_use]
    pub const fn is_include(&self) -> bool {
        self.kind.is_include()
    }

    /// Returns whether this is a `#define` directive.
    #[must_use]
    pub const fn is_define(&self) -> bool {
        self.kind.is_define()
    }

    /// Returns whether this is a `#require` directive.
    #[must_use]
    pub fn is_require(&self) -> bool {
        matches!(self.kind, DirectiveKind::Other { name, .. } if name.as_str().as_bytes() == b"require")
    }

    /// Returns the directive source span.
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        <Self as SpannedSyntax>::span(self)
    }

    /// Returns directive text borrowed from the original source.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> &'source str {
        self.text_from(ShaderSourceText::new(source))
    }

    /// Returns directive text borrowed from a typed source view.
    #[must_use]
    pub fn text_from<'source>(&self, source: ShaderSourceText<'source>) -> &'source str {
        source.slice(self.span)
    }

    /// Returns directive text borrowed from its parsed module.
    #[must_use]
    pub fn text_in<'source>(&self, module: &ShaderModule<'source>) -> &'source str {
        module.slice(self.span)
    }
}

impl SpannedSyntax for PreprocessorDirective<'_> {
    fn span(&self) -> SourceSpan {
        self.span
    }
}

/// Semantic preprocessor directive categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectiveKind<'src> {
    /// `#include` directive.
    Include(IncludeDirective<'src>),
    /// `#define` directive.
    Define(DefineDirective<'src>),
    /// Conditional directive such as `#if`, `#ifdef`, or `#endif`.
    Conditional(ConditionalDirective<'src>),
    /// Any other preprocessor directive.
    Other {
        /// Directive keyword.
        name: DirectiveName<'src>,
        /// Directive body after the keyword.
        body: DirectiveBody<'src>,
    },
}

impl<'src> DirectiveKind<'src> {
    /// Returns the directive keyword.
    #[must_use]
    pub const fn name(self) -> DirectiveName<'src> {
        match self {
            Self::Include(directive) => directive.name(),
            Self::Define(directive) => directive.name(),
            Self::Conditional(directive) => directive.name(),
            Self::Other { name, .. } => name,
        }
    }

    /// Returns the directive body after the keyword.
    #[must_use]
    pub const fn body(self) -> DirectiveBody<'src> {
        match self {
            Self::Include(directive) => directive.body(),
            Self::Define(directive) => directive.body(),
            Self::Conditional(directive) => directive.body(),
            Self::Other { body, .. } => body,
        }
    }

    /// Returns directive text without the leading `#`.
    #[must_use]
    pub const fn raw(self) -> &'src str {
        match self {
            Self::Include(directive) => directive.raw(),
            Self::Define(directive) => directive.raw(),
            Self::Conditional(directive) => directive.raw(),
            Self::Other { name, .. } => name.raw(),
        }
    }

    /// Returns whether this is an `#include` directive.
    #[must_use]
    pub const fn is_include(self) -> bool {
        matches!(self, Self::Include(_))
    }

    /// Returns whether this is a `#define` directive.
    #[must_use]
    pub const fn is_define(self) -> bool {
        matches!(self, Self::Define(_))
    }

    /// Returns whether this is a conditional directive.
    #[must_use]
    pub const fn is_conditional(self) -> bool {
        matches!(self, Self::Conditional(_))
    }

    /// Returns the typed include directive when this is `#include`.
    #[must_use]
    pub const fn include(self) -> Option<IncludeDirective<'src>> {
        match self {
            Self::Include(directive) => Some(directive),
            Self::Define(_) | Self::Conditional(_) | Self::Other { .. } => None,
        }
    }

    /// Returns the typed define directive when this is `#define`.
    #[must_use]
    pub const fn define(self) -> Option<DefineDirective<'src>> {
        match self {
            Self::Define(directive) => Some(directive),
            Self::Include(_) | Self::Conditional(_) | Self::Other { .. } => None,
        }
    }

    /// Returns the typed conditional directive when this is conditional.
    #[must_use]
    pub const fn conditional(self) -> Option<ConditionalDirective<'src>> {
        match self {
            Self::Conditional(directive) => Some(directive),
            Self::Include(_) | Self::Define(_) | Self::Other { .. } => None,
        }
    }
}

/// Parsed `#include` directive syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncludeDirective<'src> {
    /// Directive keyword.
    name: DirectiveName<'src>,
    /// Include directive body.
    body: DirectiveBody<'src>,
}

impl<'src> IncludeDirective<'src> {
    /// Returns the directive keyword.
    #[must_use]
    pub const fn name(self) -> DirectiveName<'src> {
        self.name
    }

    /// Returns the directive body.
    #[must_use]
    pub const fn body(self) -> DirectiveBody<'src> {
        self.body
    }

    /// Returns the directive text without the leading `#`.
    #[must_use]
    pub const fn raw(self) -> &'src str {
        self.name.raw()
    }

    /// Returns the quoted or angle-bracket include path text.
    #[must_use]
    pub fn path_text(self) -> &'src str {
        self.body.include_path_text().unwrap_or("")
    }

    /// Returns the include path as a domain identifier.
    ///
    /// # Errors
    ///
    /// Returns an error when the directive body is not a quoted or
    /// angle-bracket include path, or the path is invalid.
    pub fn include_path(self) -> Result<IncludePath, &'static str> {
        let path_text = self
            .body
            .include_path_text()
            .ok_or("#include expects a quoted include path")?;
        IncludePath::new(path_text).map_err(|_error| "#include path is invalid")
    }
}

/// Parsed `#define` directive syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefineDirective<'src> {
    /// Directive keyword.
    name: DirectiveName<'src>,
    /// Define directive body.
    body: DirectiveBody<'src>,
}

impl<'src> DefineDirective<'src> {
    /// Returns the directive keyword.
    #[must_use]
    pub const fn name(self) -> DirectiveName<'src> {
        self.name
    }

    /// Returns the directive body.
    #[must_use]
    pub const fn body(self) -> DirectiveBody<'src> {
        self.body
    }

    /// Returns the directive text without the leading `#`.
    #[must_use]
    pub const fn raw(self) -> &'src str {
        self.name.raw()
    }
}

/// Parsed facts from a `#define` directive body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefineDirectiveParts<'src> {
    /// Macro signature before replacement text.
    signature: DirectiveBody<'src>,
    /// Macro replacement text.
    value: DirectiveBody<'src>,
}

impl<'src> DefineDirectiveParts<'src> {
    /// Returns the macro signature.
    #[must_use]
    pub const fn signature(self) -> DirectiveBody<'src> {
        self.signature
    }

    /// Returns the macro replacement text.
    #[must_use]
    pub const fn value(self) -> DirectiveBody<'src> {
        self.value
    }
}

/// Parsed conditional preprocessor directive syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConditionalDirective<'src> {
    /// Directive keyword.
    name: DirectiveName<'src>,
    /// Conditional directive body.
    body: DirectiveBody<'src>,
}

impl<'src> ConditionalDirective<'src> {
    /// Returns the directive keyword.
    #[must_use]
    pub const fn name(self) -> DirectiveName<'src> {
        self.name
    }

    /// Returns the directive body.
    #[must_use]
    pub const fn body(self) -> DirectiveBody<'src> {
        self.body
    }

    /// Returns the directive text without the leading `#`.
    #[must_use]
    pub const fn raw(self) -> &'src str {
        self.name.raw()
    }

    /// Returns whether this is `#ifdef`.
    #[must_use]
    pub fn is_ifdef(self) -> bool {
        self.name.as_str().as_bytes() == b"ifdef"
    }

    /// Returns whether this is `#ifndef`.
    #[must_use]
    pub fn is_ifndef(self) -> bool {
        self.name.as_str().as_bytes() == b"ifndef"
    }

    /// Returns whether this is `#if`.
    #[must_use]
    pub fn is_if(self) -> bool {
        self.name.as_str().as_bytes() == b"if"
    }

    /// Returns whether this is `#elif`.
    #[must_use]
    pub fn is_elif(self) -> bool {
        self.name.as_str().as_bytes() == b"elif"
    }

    /// Returns whether this is `#else`.
    #[must_use]
    pub fn is_else(self) -> bool {
        self.name.as_str().as_bytes() == b"else"
    }

    /// Returns whether this is `#endif`.
    #[must_use]
    pub fn is_endif(self) -> bool {
        self.name.as_str().as_bytes() == b"endif"
    }
}

/// Preprocessor directive keyword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectiveName<'src> {
    /// Full directive text without the leading `#`.
    raw: &'src str,
    /// Keyword slice.
    source: &'src str,
}

impl<'src> DirectiveName<'src> {
    /// Returns directive text without the leading `#`.
    #[must_use]
    pub const fn raw(self) -> &'src str {
        self.raw
    }

    /// Returns the keyword text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }
}

/// Preprocessor directive body after the keyword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectiveBody<'src> {
    /// Body text with trailing line comments stripped.
    source: &'src str,
}

impl<'src> DirectiveBody<'src> {
    /// Creates a directive body.
    #[must_use]
    pub const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Returns body text with trailing line comments stripped.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }

    /// Returns a quoted or angle-bracket include path body without delimiters.
    #[must_use]
    pub fn include_path_text(self) -> Option<&'src str> {
        self.source
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                self.source
                    .strip_prefix('<')
                    .and_then(|value| value.strip_suffix('>'))
            })
    }
}

impl<'src> DirectiveBody<'src> {
    /// Removes a trailing `//` comment outside strings and angle includes.
    fn without_trailing_comment(self) -> &'src str {
        let mut in_quotes = false;
        let mut in_angles = false;
        let mut previous_was_escape = false;
        let mut chars = self.source.char_indices().peekable();

        while let Some((index, character)) = chars.next() {
            if character == '"' && !previous_was_escape {
                in_quotes = !in_quotes;
            }
            if !in_quotes && character == '<' {
                in_angles = true;
            }
            if !in_quotes && character == '>' {
                in_angles = false;
            }

            if !in_quotes
                && !in_angles
                && character == '/'
                && chars.peek().is_some_and(|(_, next)| *next == '/')
            {
                return self.source[..index].trim();
            }

            previous_was_escape = character == '\\' && !previous_was_escape;
            if character != '\\' {
                previous_was_escape = false;
            }
        }

        self.source.trim()
    }
}
