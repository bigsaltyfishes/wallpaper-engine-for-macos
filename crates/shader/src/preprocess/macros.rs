//! Preprocessor macro table and directive parsing.

use smol_str::SmolStr;

use crate::ShaderComboValue;

/// Macro values visible while preprocessing shader conditionals.
#[derive(Clone, Debug, Default)]
pub struct MacroTable {
    /// Macro values keyed by source names.
    values: Vec<MacroValue>,
}

impl PartialEq for MacroTable {
    fn eq(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .all(|entry| other.value(entry.name.as_str()) == Some(entry.value.as_str()))
    }
}

impl Eq for MacroTable {}

impl MacroTable {
    /// Creates an empty macro table.
    #[must_use]
    pub const fn new() -> Self {
        Self { values: Vec::new() }
    }

    /// Creates a macro table from request combo values.
    #[must_use]
    pub fn from_combos(combos: &[ShaderComboValue]) -> Self {
        let mut table = Self { values: Vec::new() };

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
        if let Some(existing) = self.values.iter_mut().find(|entry| entry.has_name(name)) {
            existing.replace(value);
        } else {
            self.values.push(MacroValue {
                name: SmolStr::new(name),
                value: SmolStr::new(value),
            });
        }
    }

    /// Returns a macro value by name.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|entry| entry.has_name(name))
            .map(|entry| entry.value.as_str())
    }

    /// Returns whether a macro has been defined.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.values.iter().any(|entry| entry.has_name(name))
    }
}

/// Object-like macro value visible to conditional preprocessing.
#[derive(Clone, Debug, Eq, PartialEq)]
struct MacroValue {
    /// Macro identifier.
    name: SmolStr,
    /// Replacement value.
    value: SmolStr,
}

impl MacroValue {
    /// Returns whether this value belongs to the requested name.
    fn has_name(&self, name: &str) -> bool {
        self.name.as_str() == name
    }

    /// Replaces the macro value.
    fn replace(&mut self, value: &str) {
        self.value = SmolStr::new(value);
    }
}

/// Valid preprocessor macro identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacroName<'src> {
    /// Borrowed identifier text.
    source: &'src str,
}

impl<'src> MacroName<'src> {
    /// Parses and validates a preprocessor macro identifier.
    pub(super) fn parse(source: &'src str) -> Result<Self, &'static str> {
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

    /// Returns the identifier text.
    pub(super) const fn as_str(self) -> &'src str {
        self.source
    }
}
