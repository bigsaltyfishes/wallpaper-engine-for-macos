//! Preprocessor macro table and directive parsing.

use std::collections::BTreeMap;

use crate::{ShaderComboValue, syntax::PreprocessorDirective};

/// Source prelude that exposes request combos to backend preprocessing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct MacroPrelude {
    /// Macro definition lines.
    source: String,
}

impl From<&[ShaderComboValue]> for MacroPrelude {
    fn from(combos: &[ShaderComboValue]) -> Self {
        let mut source = String::new();
        for combo in combos {
            source.push_str("#define ");
            source.push_str(&combo.name().as_str().to_ascii_uppercase());
            source.push(' ');
            source.push_str(combo.value());
            source.push('\n');
        }
        Self { source }
    }
}

impl MacroPrelude {
    /// Prepends this prelude to a stage source when non-empty.
    pub(super) fn prepend_to(&self, source: &mut String) {
        if self.source.is_empty() {
            return;
        }
        let mut output = String::with_capacity(self.source.len() + source.len());
        output.push_str(&self.source);
        output.push_str(source);
        *source = output;
    }
}

/// Macro values visible while preprocessing shader conditionals.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacroTable {
    /// Macro values keyed by normalized source names.
    values: BTreeMap<String, String>,
}

impl MacroTable {
    /// Creates an empty macro table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Creates a macro table from request combo values.
    #[must_use]
    pub fn from_combos(combos: &[ShaderComboValue]) -> Self {
        let mut table = Self {
            values: BTreeMap::new(),
        };

        for (name, value) in [("GLSL", "1"), ("HLSL", "0")] {
            table.define(name, value);
        }
        for combo in combos {
            table.define(combo.name().as_str(), combo.value());
            table.define(&combo.name().as_str().to_ascii_uppercase(), combo.value());
        }

        table
    }

    /// Defines or replaces a macro value.
    pub fn define(&mut self, name: &str, value: &str) {
        let _old = self.values.insert(name.to_owned(), value.to_owned());
    }

    /// Returns a macro value by name.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Returns whether a macro has been defined.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}

/// Parsed `#define` directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DefineDirective<'src> {
    /// Macro name being defined.
    name: MacroName<'src>,
    /// Replacement text for the macro.
    value: &'src str,
}

impl<'src> DefineDirective<'src> {
    /// Returns the macro name.
    pub(super) const fn name(self) -> MacroName<'src> {
        self.name
    }

    /// Returns the macro replacement text.
    pub(super) const fn value(self) -> &'src str {
        self.value
    }
}

impl<'src> TryFrom<PreprocessorDirective<'src>> for DefineDirective<'src> {
    type Error = &'static str;

    fn try_from(directive: PreprocessorDirective<'src>) -> Result<Self, Self::Error> {
        let Some(parts) = directive.define_parts()? else {
            return Err("#define expects a macro name");
        };
        let signature = MacroSignature::try_from(directive)
            .map_err(|_error| "#define macro name is invalid")?;

        Ok(Self {
            name: signature.name(),
            value: parts.value().as_str(),
        })
    }
}

/// Parsed object-like or function-like macro signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacroSignature<'src> {
    /// Macro name in the signature.
    name: MacroName<'src>,
}

impl<'src> MacroSignature<'src> {
    /// Returns the macro name.
    const fn name(self) -> MacroName<'src> {
        self.name
    }
}

impl<'src> TryFrom<PreprocessorDirective<'src>> for MacroSignature<'src> {
    type Error = &'static str;

    fn try_from(directive: PreprocessorDirective<'src>) -> Result<Self, Self::Error> {
        let Some(parts) = directive.define_parts()? else {
            return Err("#define macro name is invalid");
        };
        let signature = parts.signature();
        let name = signature
            .as_str()
            .split_once('(')
            .map_or(signature.as_str(), |(name, _parameters)| name);
        Ok(Self {
            name: MacroName::try_from(name)?,
        })
    }
}

/// Valid preprocessor macro identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacroName<'src> {
    /// Borrowed identifier text.
    source: &'src str,
}

impl<'src> MacroName<'src> {
    /// Returns the identifier text.
    pub(super) const fn as_str(self) -> &'src str {
        self.source
    }
}

impl<'src> TryFrom<&'src str> for MacroName<'src> {
    type Error = &'static str;

    fn try_from(source: &'src str) -> Result<Self, Self::Error> {
        let trimmed = source.trim();
        let mut chars = trimmed.chars();
        let Some(first) = chars.next() else {
            return Err("conditional expects a single macro name");
        };

        if !(first == '_' || first.is_ascii_alphabetic())
            || !chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            return Err("conditional expects a single macro name");
        }

        Ok(Self { source: trimmed })
    }
}
