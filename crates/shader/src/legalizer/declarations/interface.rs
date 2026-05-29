//! Stage interface declaration planning and emission models.

use std::{borrow::Cow, fmt::Write as _};

use super::{
    super::emission::SourceEmitter, DeclarationEntry, PlannedDeclaration, types::LegacyTypeName,
};
use crate::{
    ShaderError, ShaderResult, ShaderStageKind, SourceSpan, layout::InterfaceLocation,
    syntax::TopLevelQualifier,
};

/// Program-level stage interface layout edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StageInterfaceLayout<'src> {
    /// Explicit locations for existing stage inputs/outputs.
    bindings: Vec<StageInterfaceLayoutBinding<'src>>,
    /// Extra stage interfaces that must be emitted by this stage.
    synthesized: Vec<SynthesizedStageInterface<'src>>,
}

impl<'src> StageInterfaceLayout<'src> {
    /// Creates a program-level interface layout.
    #[must_use]
    pub(crate) fn new(
        bindings: Vec<StageInterfaceLayoutBinding<'src>>,
        synthesized: Vec<SynthesizedStageInterface<'src>>,
    ) -> Self {
        Self {
            bindings,
            synthesized,
        }
    }

    /// Returns the assigned binding for a source interface declaration.
    pub(super) fn binding_for(
        &self,
        interface: &StageInterface<'_>,
    ) -> Option<&StageInterfaceLayoutBinding<'src>> {
        self.bindings
            .iter()
            .find(|binding| binding.matches(interface))
    }

    /// Appends synthesized interfaces for `stage` after source declarations.
    pub(super) fn push_synthesized_stage_interfaces(
        &self,
        stage: ShaderStageKind,
        entries: &mut Vec<DeclarationEntry<'src>>,
    ) -> ShaderResult<()> {
        for interface in self
            .synthesized
            .iter()
            .filter(|interface| interface.stage == stage)
        {
            entries.push(DeclarationEntry {
                span: SourceSpan::new(0, 0)?,
                kind: PlannedDeclaration::Interface(StageInterface {
                    direction: interface.direction,
                    ty: interface.ty,
                    name: interface.name.to_cow(),
                    array_suffix: interface.array_suffix,
                    location: Some(InterfaceLocation::new(interface.location)?),
                    local_copy: false,
                    initializer: interface.initializer,
                }),
            });
        }
        Ok(())
    }
}

/// Explicit location assignment for a source interface declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageInterfaceLayoutBinding<'src> {
    /// Interface direction within the stage.
    pub(crate) direction: InterfaceDirection,
    /// Source variable name.
    pub(crate) name: &'src str,
    /// Optional type override emitted instead of the source declaration type.
    pub(crate) ty: Option<&'src str>,
    /// Assigned location.
    pub(crate) location: u32,
}

impl StageInterfaceLayoutBinding<'_> {
    /// Returns whether this assignment applies to `interface`.
    fn matches(&self, interface: &StageInterface<'_>) -> bool {
        self.direction == interface.direction && self.name == interface.name.as_ref()
    }
}

/// Synthesized interface declaration emitted by program-level assembly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesizedStageInterface<'src> {
    /// Stage receiving the extra interface.
    pub(crate) stage: ShaderStageKind,
    /// Interface direction within the stage.
    pub(crate) direction: InterfaceDirection,
    /// Interface type.
    pub(crate) ty: &'src str,
    /// Interface variable name.
    pub(crate) name: SynthesizedName<'src>,
    /// Optional array suffix following the interface name.
    pub(crate) array_suffix: Option<&'src str>,
    /// Assigned location.
    pub(crate) location: u32,
    /// Optional main prelude assignment for synthesized outputs.
    pub(crate) initializer: Option<StageInterfaceInitializer>,
}

/// Emitted name for a synthesized stage interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynthesizedName<'src> {
    /// Borrowed source name when no collision repair is needed.
    Source(&'src str),
    /// Generated identifier when the source name collides with a stage global.
    Generated(String),
}

impl<'src> SynthesizedName<'src> {
    /// Returns this name in the storage form used by emitted interfaces.
    fn to_cow(&self) -> Cow<'src, str> {
        match self {
            Self::Source(name) => Cow::Borrowed(name),
            Self::Generated(name) => Cow::Owned(name.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Source declaration for the legacy macro-aliased vertex position variable.
pub(super) struct MacroAliasedPositionDeclaration {
    /// Source qualifier that must not reach backend GLSL.
    pub(super) qualifier: TopLevelQualifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Stage input or output declaration with assigned layout metadata.
pub struct StageInterface<'src> {
    /// Whether the interface is an input or output for this stage.
    pub(super) direction: InterfaceDirection,
    /// Source type name.
    pub(super) ty: &'src str,
    /// Source variable name.
    pub(crate) name: Cow<'src, str>,
    /// Optional array suffix following the interface name.
    pub(super) array_suffix: Option<&'src str>,
    /// Allocated interface location.
    pub(super) location: Option<InterfaceLocation>,
    /// Whether mutable input usage requires an `_we_in_` backing variable.
    pub(super) local_copy: bool,
    /// Optional main prelude assignment for synthesized outputs.
    pub(super) initializer: Option<StageInterfaceInitializer>,
}

impl StageInterface<'_> {
    /// Marks this input for local mutable-copy emission.
    pub(crate) fn use_local_copy(&mut self) {
        self.local_copy = true;
    }

    /// Emits the generated interface declaration.
    pub(crate) fn emit(&self, output: &mut String) -> ShaderResult<()> {
        let location = self.location.ok_or_else(|| {
            ShaderError::invalid_request("interface location was not assigned before emission")
        })?;
        let qualifier = match self.direction {
            InterfaceDirection::Input => "in",
            InterfaceDirection::Output => "out",
        };
        let name = self.emitted_name();
        writeln!(
            output,
            "layout(location = {}) {} {} {}{};",
            location.index(),
            qualifier,
            LegacyTypeName::new(self.ty).glsl(),
            name.as_str(),
            self.array_suffix.unwrap_or_default()
        )
        .map_err(SourceEmitter::write_error)
    }

    /// Returns the name used in generated interface declarations.
    fn emitted_name(&self) -> String {
        if self.local_copy {
            let mut name = String::from("_we_in_");
            name.push_str(self.name.as_ref());
            name
        } else {
            self.name.to_string()
        }
    }

    /// Emits the main-function local copy for mutable vertex inputs.
    pub(crate) fn emit_local_copy(&self, output: &mut String) -> ShaderResult<()> {
        if let Some(initializer) = self.initializer {
            writeln!(
                output,
                "    {} = {};",
                self.name.as_ref(),
                initializer.expression(self.ty)
            )
            .map_err(SourceEmitter::write_error)?;
        }
        if !self.local_copy {
            return Ok(());
        }
        writeln!(
            output,
            "    {} {} = {};",
            LegacyTypeName::new(self.ty).glsl(),
            self.name.as_ref(),
            self.emitted_name()
        )
        .map_err(SourceEmitter::write_error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Direction of a generated stage interface declaration.
pub enum InterfaceDirection {
    /// Shader stage input.
    Input,
    /// Shader stage output.
    Output,
}

/// Synthesized interface initialization strategy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageInterfaceInitializer {
    /// Assign a zero value matching the declared scalar/vector type.
    Zero,
}

impl StageInterfaceInitializer {
    /// Returns the GLSL expression used for this initializer.
    fn expression(self, ty: &str) -> String {
        match self {
            Self::Zero => match LegacyTypeName::new(ty).glsl() {
                "float" => "0.0".to_owned(),
                "vec2" => "vec2(0.0)".to_owned(),
                "vec3" => "vec3(0.0)".to_owned(),
                "vec4" => "vec4(0.0)".to_owned(),
                glsl_ty => format!("{glsl_ty}(0)"),
            },
        }
    }
}
