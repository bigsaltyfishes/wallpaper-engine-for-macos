//! Final source emission from generated declarations and collected fixups.

use std::fmt::Write as _;

use super::{
    declarations::DeclarationPlan,
    expressions::ExpressionRenderer,
    fixups::{FixupReplacement, FixupSet},
};
use crate::{ShaderError, ShaderResult, syntax::ShaderModule};

/// Emits final source from generated declarations and ordered fixups.
pub(super) struct SourceEmitter<'module, 'src> {
    /// Parsed shader module being emitted.
    pub(super) module: &'module ShaderModule<'src>,
    /// Planned generated declarations.
    pub(super) declarations: DeclarationPlan<'src>,
    /// Source edits collected by semantic analysis.
    pub(super) fixups: FixupSet,
}

impl SourceEmitter<'_, '_> {
    /// Emits complete GLSL source with header, declarations, and rewritten
    /// original body.
    pub(super) fn emit(mut self) -> ShaderResult<String> {
        self.fixups
            .insert_main_prelude(self.module, &self.declarations)?;
        let mut output = String::with_capacity(self.module.source().as_str().len() + 256);
        let leading_defines = LeadingObjectDefines::from(self.module.source().as_str());
        writeln!(output, "#version 450").map_err(Self::write_error)?;
        writeln!(output, "precision highp float;").map_err(Self::write_error)?;
        writeln!(output).map_err(Self::write_error)?;
        self.emit_generated_declarations(&mut output, leading_defines)?;
        self.declarations
            .emit_compatibility_functions(&mut output)?;
        self.emit_original_with_fixups(&mut output)?;
        if !output.ends_with('\n') {
            output.push('\n');
        }
        Ok(output)
    }

    /// Emits generated resource and interface declarations.
    fn emit_generated_declarations(
        &self,
        output: &mut String,
        leading_defines: LeadingObjectDefines<'_>,
    ) -> ShaderResult<()> {
        for texture in self.declarations.textures() {
            texture.emit(output)?;
        }
        for sampler in self.declarations.texture_samplers() {
            sampler.emit(output)?;
        }
        if self.declarations.has_textures() {
            writeln!(output).map_err(Self::write_error)?;
        }

        if let Some(block) = self.declarations.uniform_block() {
            block.emit_with_array_suffix_resolver(output, |suffix| {
                leading_defines.resolved_array_suffix(suffix)
            })?;
            writeln!(output).map_err(Self::write_error)?;
        }

        for interface in self.declarations.stage_interfaces() {
            interface.emit(output)?;
        }
        if self.declarations.has_fragment_output() {
            writeln!(output, "layout(location = 0) out vec4 _we_FragColor;")
                .map_err(Self::write_error)?;
        }
        if self.declarations.stage_interfaces().next().is_some()
            || self.declarations.has_fragment_output()
        {
            writeln!(output).map_err(Self::write_error)?;
        }

        Ok(())
    }

    /// Copies original source into output while applying ordered fixups.
    fn emit_original_with_fixups(&mut self, output: &mut String) -> ShaderResult<()> {
        let source = self.module.source().as_str();
        let mut copied = 0usize;
        let fixups = self.fixups.ordered()?;
        let renderer = ExpressionRenderer {
            source: self.module.source(),
            fixups,
        };
        for (index, fixup) in fixups.iter().enumerate() {
            if FixupSet::is_expression_child(fixups, index) {
                continue;
            }
            output.push_str(&source[copied..fixup.span().start()]);
            match fixup.replacement() {
                FixupReplacement::Text(replacement) => output.push_str(replacement),
                FixupReplacement::Expression(replacement) => {
                    output.push_str(&renderer.render_replacement(replacement, index)?);
                }
            }
            copied = fixup.span().end();
        }
        output.push_str(&source[copied..]);
        Ok(())
    }

    /// Converts an infallible string formatting error into a shader error.
    pub(super) fn write_error(error: std::fmt::Error) -> ShaderError {
        ShaderError::invalid_request(format!("failed to emit legalized source: {error}"))
    }
}

/// Leading object-like `#define`s available to generated declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LeadingObjectDefines<'src> {
    /// Source slice containing the initial directive prelude.
    source: &'src str,
}

impl<'src> From<&'src str> for LeadingObjectDefines<'src> {
    /// Extracts the contiguous leading block of macro definitions inserted by
    /// preprocessing.
    fn from(source: &'src str) -> Self {
        let mut cursor = 0usize;
        let mut end = 0usize;

        while cursor < source.len() {
            let line_end = source[cursor..]
                .find('\n')
                .map_or(source.len(), |offset| cursor + offset + 1);
            let line = source[cursor..line_end].trim_end_matches(['\n', '\r']);
            let trimmed = line.trim_start();
            if !trimmed
                .strip_prefix("#define")
                .is_some_and(|rest| rest.chars().next().is_some_and(char::is_whitespace))
            {
                break;
            }
            end = line_end;
            cursor = line_end;
        }

        Self {
            source: &source[..end],
        }
    }
}

impl LeadingObjectDefines<'_> {
    /// Returns a concrete array suffix for `[IDENT]` when the identifier is a
    /// leading macro with a simple literal value.
    fn resolved_array_suffix(self, suffix: &str) -> Option<String> {
        let identifier = suffix.strip_prefix('[')?.strip_suffix(']')?.trim();
        if identifier.is_empty() || Self::identifier_len(identifier) != identifier.len() {
            return None;
        }

        for line in self.source.lines() {
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix("#define")?.trim_start();
            let name_len = Self::identifier_len(rest);
            if rest.get(..name_len)? != identifier {
                continue;
            }
            let value = rest[name_len..].trim_start();
            if value.is_empty()
                || !value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '+' | '-')
                })
            {
                continue;
            }
            return Some(format!("[{value}]"));
        }

        None
    }

    /// Returns the byte length of a leading GLSL identifier.
    fn identifier_len(text: &str) -> usize {
        text.char_indices()
            .take_while(|(index, character)| {
                if *index == 0 {
                    character.is_ascii_alphabetic() || *character == '_'
                } else {
                    character.is_ascii_alphanumeric() || *character == '_'
                }
            })
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or_default()
    }
}
