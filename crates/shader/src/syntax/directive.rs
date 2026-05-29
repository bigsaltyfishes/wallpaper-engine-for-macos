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
            "if" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::If,
                name,
                body,
            }),
            "ifdef" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::Ifdef,
                name,
                body,
            }),
            "ifndef" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::Ifndef,
                name,
                body,
            }),
            "elif" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::Elif,
                name,
                body,
            }),
            "else" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::Else,
                name,
                body,
            }),
            "endif" => DirectiveKind::Conditional(ConditionalDirective {
                kind: ConditionalDirectiveKind::Endif,
                name,
                body,
            }),
            "require" => DirectiveKind::Require(RequireDirective { name, body }),
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

        let bytes = body.as_bytes();
        let Some(first) = bytes.first().copied() else {
            return Err("#define expects a macro name");
        };
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return Err("#define expects a macro name");
        }
        let mut signature_end = 1;
        while bytes
            .get(signature_end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            signature_end += 1;
        }
        if bytes.get(signature_end) == Some(&b'(') {
            let mut depth = 0usize;
            while let Some(byte) = bytes.get(signature_end) {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth = depth.checked_sub(1).ok_or("#define expects a macro name")?;
                        signature_end += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    _ => {}
                }
                signature_end += 1;
            }
            if depth != 0 {
                return Err("#define expects a macro name");
            }
        } else if body[signature_end..]
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            return Err("#define expects a macro name");
        }
        let signature = &body[..signature_end];
        let value = body[signature_end..].trim();
        let has_explicit_value = !value.is_empty();
        let value = if has_explicit_value { value } else { "1" };

        Ok(Some(DefineDirectiveParts {
            signature: DirectiveBody::new(signature),
            value: DirectiveBody::new(value),
            has_explicit_value,
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
        matches!(self.kind, DirectiveKind::Require(_))
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

#[cfg(test)]
mod tests {
    use super::PreprocessorDirective;
    use crate::SourceSpan;

    #[test]
    fn define_parts_rejects_adjacent_invalid_delimiter_after_name() {
        let directive =
            PreprocessorDirective::from_token_text("#define FOO-BAR 1", SourceSpan::default());

        let error = directive
            .define_parts()
            .expect_err("adjacent invalid delimiter should reject macro name");

        assert_eq!(error, "#define expects a macro name");
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
    /// Wallpaper Engine `#require` directive.
    Require(RequireDirective<'src>),
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
            Self::Require(directive) => directive.name(),
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
            Self::Require(directive) => directive.body(),
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
            Self::Require(directive) => directive.raw(),
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
            Self::Define(_) | Self::Conditional(_) | Self::Require(_) | Self::Other { .. } => None,
        }
    }

    /// Returns the typed define directive when this is `#define`.
    #[must_use]
    pub const fn define(self) -> Option<DefineDirective<'src>> {
        match self {
            Self::Define(directive) => Some(directive),
            Self::Include(_) | Self::Conditional(_) | Self::Require(_) | Self::Other { .. } => None,
        }
    }

    /// Returns the typed conditional directive when this is conditional.
    #[must_use]
    pub const fn conditional(self) -> Option<ConditionalDirective<'src>> {
        match self {
            Self::Conditional(directive) => Some(directive),
            Self::Include(_) | Self::Define(_) | Self::Require(_) | Self::Other { .. } => None,
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
    /// Whether the source directive had non-empty replacement text.
    has_explicit_value: bool,
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

    /// Returns the byte offset of the replacement text within the directive
    /// token text.
    #[must_use]
    pub fn value_offset_in(self, directive_text: &str) -> Option<usize> {
        let value = self.value().as_str();
        if value.is_empty() {
            return None;
        }
        let text_start = directive_text.as_ptr().addr();
        let value_start = value.as_ptr().addr();
        (text_start..=text_start + directive_text.len())
            .contains(&value_start)
            .then_some(value_start - text_start)
    }

    /// Returns whether the source directive had non-empty replacement text.
    #[must_use]
    pub const fn has_explicit_value(self) -> bool {
        self.has_explicit_value
    }

    /// Returns the macro identifier before any function-like parameter list.
    #[must_use]
    pub fn name_text(self) -> &'src str {
        self.signature()
            .as_str()
            .split_once('(')
            .map_or(self.signature().as_str(), |(name, _parameters)| name)
    }

    /// Returns the macro identifier when this is an object-like definition.
    #[must_use]
    pub fn object_like_name_text(self) -> Option<&'src str> {
        let signature = self.signature().as_str();
        (!signature.is_empty() && !signature.contains('(')).then_some(signature)
    }

    /// Returns a simple replacement spelling without token separators.
    #[must_use]
    pub fn simple_replacement_text(self) -> Option<&'src str> {
        let value = self.value().as_str();
        (!value.is_empty()
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '+' | '-')
            }))
        .then_some(value)
    }
}

/// Parsed Wallpaper Engine `#require` directive syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequireDirective<'src> {
    /// Directive keyword.
    name: DirectiveName<'src>,
    /// Require directive body.
    body: DirectiveBody<'src>,
}

impl<'src> RequireDirective<'src> {
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

/// Parsed conditional preprocessor directive syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConditionalDirective<'src> {
    /// Specific conditional directive keyword.
    kind: ConditionalDirectiveKind,
    /// Directive keyword.
    name: DirectiveName<'src>,
    /// Conditional directive body.
    body: DirectiveBody<'src>,
}

impl<'src> ConditionalDirective<'src> {
    /// Returns the specific conditional directive keyword.
    #[must_use]
    pub const fn kind(self) -> ConditionalDirectiveKind {
        self.kind
    }

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
        self.kind == ConditionalDirectiveKind::Ifdef
    }

    /// Returns whether this is `#ifndef`.
    #[must_use]
    pub fn is_ifndef(self) -> bool {
        self.kind == ConditionalDirectiveKind::Ifndef
    }

    /// Returns whether this is `#if`.
    #[must_use]
    pub fn is_if(self) -> bool {
        self.kind == ConditionalDirectiveKind::If
    }

    /// Returns whether this is `#elif`.
    #[must_use]
    pub fn is_elif(self) -> bool {
        self.kind == ConditionalDirectiveKind::Elif
    }

    /// Returns whether this is `#else`.
    #[must_use]
    pub fn is_else(self) -> bool {
        self.kind == ConditionalDirectiveKind::Else
    }

    /// Returns whether this is `#endif`.
    #[must_use]
    pub fn is_endif(self) -> bool {
        self.kind == ConditionalDirectiveKind::Endif
    }
}

/// Specific conditional preprocessor directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionalDirectiveKind {
    /// `#if`.
    If,
    /// `#ifdef`.
    Ifdef,
    /// `#ifndef`.
    Ifndef,
    /// `#elif`.
    Elif,
    /// `#else`.
    Else,
    /// `#endif`.
    Endif,
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
