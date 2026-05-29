//! Stage interface declaration planning and emission models.

use std::{borrow::Cow, fmt::Write as _};

use smol_str::SmolStr;

use super::{
    super::emission::SourceEmitter, DeclarationEntry, PlannedDeclaration, types::LegacyTypeName,
};
use crate::{
    ShaderError, ShaderResult, ShaderStageKind, SourceSpan, layout::InterfaceLocation,
    syntax::TopLevelQualifier,
};

/// Program-level stage interface layout edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StageInterfaceLayout {
    /// Explicit locations for existing stage inputs/outputs.
    bindings: Vec<StageInterfaceLayoutBinding>,
    /// Extra stage interfaces that must be emitted by this stage.
    synthesized: Vec<SynthesizedStageInterface>,
}

impl StageInterfaceLayout {
    /// Creates a program-level interface layout.
    #[must_use]
    pub(crate) fn new(
        bindings: Vec<StageInterfaceLayoutBinding>,
        synthesized: Vec<SynthesizedStageInterface>,
    ) -> Self {
        Self {
            bindings,
            synthesized,
        }
    }

    /// Returns the assigned binding for a source interface declaration.
    #[must_use]
    pub(crate) fn binding_for(
        &self,
        interface: &StageInterface<'_>,
    ) -> Option<&StageInterfaceLayoutBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.matches(interface))
    }

    /// Appends synthesized interfaces for `stage` after source declarations.
    pub(crate) fn push_synthesized_stage_interfaces(
        &self,
        stage: ShaderStageKind,
        entries: &mut Vec<DeclarationEntry<'_>>,
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
                    ty: Cow::Owned(interface.ty.to_string()),
                    name: Cow::Owned(interface.name.to_string()),
                    array_suffix: interface
                        .array_suffix
                        .as_ref()
                        .map(|suffix| Cow::Owned(suffix.to_string())),
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StageInterfaceLayoutBinding {
    /// Interface direction within the stage.
    pub direction: InterfaceDirection,
    /// Source variable name.
    pub name: SmolStr,
    /// Optional type override emitted instead of the source declaration type.
    pub ty: Option<SmolStr>,
    /// Assigned location.
    pub location: u32,
}

impl StageInterfaceLayoutBinding {
    /// Returns whether this assignment applies to `interface`.
    fn matches(&self, interface: &StageInterface<'_>) -> bool {
        self.direction == interface.direction && self.name == interface.name.as_ref()
    }
}

/// Synthesized interface declaration emitted by program-level assembly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SynthesizedStageInterface {
    /// Stage receiving the extra interface.
    pub stage: ShaderStageKind,
    /// Interface direction within the stage.
    pub direction: InterfaceDirection,
    /// Interface type.
    pub ty: SmolStr,
    /// Interface variable name.
    pub name: SmolStr,
    /// Optional array suffix following the interface name.
    pub array_suffix: Option<SmolStr>,
    /// Assigned location.
    pub location: u32,
    /// Optional main prelude assignment for synthesized outputs.
    pub initializer: Option<StageInterfaceInitializer>,
}

/// Source declaration for the legacy macro-aliased vertex position variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacroAliasedPositionDeclaration {
    /// Source qualifier that must not reach backend GLSL.
    pub qualifier: TopLevelQualifier,
}

/// Stage input or output declaration with assigned layout metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StageInterface<'src> {
    /// Whether the interface is an input or output for this stage.
    pub direction: InterfaceDirection,
    /// Source type name.
    pub ty: Cow<'src, str>,
    /// Source variable name.
    pub name: Cow<'src, str>,
    /// Optional array suffix following the interface name.
    pub array_suffix: Option<Cow<'src, str>>,
    /// Allocated interface location.
    pub location: Option<InterfaceLocation>,
    /// Whether mutable input usage requires an `_we_in_` backing variable.
    pub local_copy: bool,
    /// Optional main prelude assignment for synthesized outputs.
    pub initializer: Option<StageInterfaceInitializer>,
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
            LegacyTypeName::new(self.ty.as_ref()).glsl(),
            name.as_str(),
            self.array_suffix.as_deref().unwrap_or_default()
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
                initializer.expression(self.ty.as_ref())
            )
            .map_err(SourceEmitter::write_error)?;
        }
        if !self.local_copy {
            return Ok(());
        }
        writeln!(
            output,
            "    {} {} = {};",
            LegacyTypeName::new(self.ty.as_ref()).glsl(),
            self.name.as_ref(),
            self.emitted_name()
        )
        .map_err(SourceEmitter::write_error)
    }
}

/// Direction of a generated stage interface declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InterfaceDirection {
    /// Shader stage input.
    Input,
    /// Shader stage output.
    Output,
}

/// Synthesized interface initialization strategy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StageInterfaceInitializer {
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
