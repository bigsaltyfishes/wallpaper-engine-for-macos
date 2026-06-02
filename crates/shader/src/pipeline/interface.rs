use smol_str::SmolStr;

use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, ShaderStageKind, SourceSpan,
    legalize::{
        InterfaceDirection, LegacyTypeName, StageInterfaceInitializer, StageInterfaceLayout,
        StageInterfaceLayoutBinding, SynthesizedStageInterface,
    },
    pipeline::inputs::ProgramStageInput,
    syntax::{InterfaceUseFacts, InterfaceUseQuery, ShaderModule, SyntaxItem, TopLevelQualifier},
};

/// Program-level vertex/fragment interface summary.
#[derive(Debug, Default)]
pub struct ProgramInterface {
    /// Vertex stage outputs in the location order the legalizer will emit.
    vertex_outputs: Vec<StageInterfaceBinding>,
    /// Fragment stage inputs in the location order the legalizer will emit.
    fragment_inputs: Vec<StageInterfaceBinding>,
    /// Vertex-stage usage summaries keyed by output varying name.
    vertex_output_uses: Vec<InterfaceUseFacts>,
    /// Fragment-stage usage summaries keyed by input varying name.
    fragment_input_uses: Vec<InterfaceUseFacts>,
}

impl ProgramInterface {
    /// Validates and builds a program-level interface layout while avoiding
    /// synthesized declaration name collisions with stage globals.
    ///
    /// # Errors
    ///
    /// Returns an error when cross-stage varyings are duplicated within one
    /// stage or have incompatible types between vertex outputs and fragment
    /// inputs.
    pub fn validate_with_names(
        &self,
        names: &StageGlobalNames,
    ) -> ShaderResult<ProgramInterfaceLayout> {
        if let Some(diagnostic) = self.first_duplicate_diagnostic() {
            return Err(Self::error(diagnostic));
        }
        if let Some(diagnostic) = self.first_incompatible_type_diagnostic() {
            return Err(Self::error(diagnostic));
        }
        Ok(ProgramInterfaceLayout::new(self, names))
    }

    /// Builds a codegen error for program-interface diagnostics.
    fn error(diagnostic: ShaderDiagnostic) -> ShaderError {
        ShaderError::Codegen {
            diagnostics: Box::from([diagnostic]),
        }
    }

    /// Finds duplicate cross-stage declarations inside a single stage.
    fn first_duplicate_diagnostic(&self) -> Option<ShaderDiagnostic> {
        Self::first_duplicate(&self.vertex_outputs)
            .or_else(|| Self::first_duplicate(&self.fragment_inputs))
    }

    /// Finds the first duplicate binding in declaration order.
    fn first_duplicate(bindings: &[StageInterfaceBinding]) -> Option<ShaderDiagnostic> {
        bindings.iter().enumerate().find_map(|(index, binding)| {
            bindings[..index]
                .iter()
                .any(|previous| previous.name == binding.name)
                .then(|| {
                    binding.diagnostic(format!(
                        "{:?} cross-stage varying `{}` is declared more than once",
                        binding.stage, binding.name
                    ))
                })
        })
    }

    /// Finds the first same-name declaration with incompatible types.
    fn first_incompatible_type_diagnostic(&self) -> Option<ShaderDiagnostic> {
        self.vertex_outputs.iter().find_map(|output| {
            let input = self
                .fragment_inputs
                .iter()
                .find(|input| input.name == output.name)?;
            let vertex_uses = self.vertex_uses(output.name.as_str());
            let fragment_uses = self.fragment_uses(input.name.as_str());
            (!output.is_compatible_with(input, vertex_uses, fragment_uses)).then(|| {
                input.diagnostic(format!(
                    "cross-stage varying `{}` type mismatch: vertex outputs {} but fragment \
                     inputs {}",
                    output.name,
                    output.glsl_ty(),
                    input.glsl_ty()
                ))
            })
        })
    }

    /// Returns usage facts for the first vertex output with `name`.
    fn vertex_uses(&self, name: &str) -> Option<&InterfaceUseFacts> {
        self.vertex_output_uses
            .iter()
            .find(|uses| uses.name() == name)
    }

    /// Returns usage facts for the first fragment input with `name`.
    fn fragment_uses(&self, name: &str) -> Option<&InterfaceUseFacts> {
        self.fragment_input_uses
            .iter()
            .find(|uses| uses.name() == name)
    }
}

impl ProgramInterface {
    /// Constructs cross-stage interface facts from parsed stage inputs.
    #[inline]
    #[allow(clippy::single_call_fn)]
    #[must_use]
    pub fn new(stages: &[ProgramStageInput<'_>]) -> Self {
        let mut interface = Self::default();
        for stage in stages {
            match stage.stage.kind() {
                ShaderStageKind::Vertex => {
                    let outputs = StageInterfaceBinding::collect_from_module(&stage.module)
                        .into_iter()
                        .filter(|binding| {
                            matches!(
                                binding.qualifier,
                                TopLevelQualifier::Varying | TopLevelQualifier::Out
                            ) && binding.name != "_ww_sv_position"
                        })
                        .collect::<Vec<_>>();
                    for output in &outputs {
                        if let Some(query) = output.interface_use_query() {
                            interface
                                .vertex_output_uses
                                .push(stage.module.interface_use_facts(query));
                        }
                    }
                    interface.vertex_outputs.extend(outputs);
                }
                ShaderStageKind::Fragment => {
                    let inputs = StageInterfaceBinding::collect_from_module(&stage.module)
                        .into_iter()
                        .filter(|binding| {
                            matches!(
                                binding.qualifier,
                                TopLevelQualifier::Varying | TopLevelQualifier::In
                            ) && binding.name != "_ww_sv_position"
                        })
                        .collect::<Vec<_>>();
                    for input in &inputs {
                        if let Some(query) = input.interface_use_query() {
                            interface
                                .fragment_input_uses
                                .push(stage.module.interface_use_facts(query));
                        }
                    }
                    interface.fragment_inputs.extend(inputs);
                }
            }
        }
        interface
    }
}

/// Stage-local global names that synthesized declarations must not reuse.
#[derive(Debug, Default)]
pub struct StageGlobalNames {
    /// Vertex-stage top-level declaration names.
    vertex: Vec<SmolStr>,
}

impl StageGlobalNames {
    /// Constructs vertex-stage global names from parsed stage inputs.
    #[inline]
    #[allow(clippy::single_call_fn)]
    #[must_use]
    pub fn new(stages: &[ProgramStageInput<'_>]) -> Self {
        let mut names = Self::default();
        for stage in stages {
            if stage.stage.kind() != ShaderStageKind::Vertex {
                continue;
            }
            for item in stage.module.items() {
                let SyntaxItem::Declaration(declaration) = item else {
                    continue;
                };
                let Some(name) = declaration.declaration_name_in(&stage.module) else {
                    continue;
                };
                let name = name.as_str();
                if !names.vertex.iter().any(|existing| existing == name) {
                    names.vertex.push(SmolStr::new(name));
                }
            }
        }
        names
    }
}

/// Program-level location layout for cross-stage interfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramInterfaceLayout {
    /// Vertex stage layout.
    vertex: StageInterfaceLayout,
    /// Fragment stage layout.
    fragment: StageInterfaceLayout,
}

impl ProgramInterfaceLayout {
    /// Returns the stage-local layout for `stage`.
    #[must_use]
    pub(crate) fn layout_for_stage(&self, stage: ShaderStageKind) -> StageInterfaceLayout {
        match stage {
            ShaderStageKind::Vertex => self.vertex.clone(),
            ShaderStageKind::Fragment => self.fragment.clone(),
        }
    }
}

impl ProgramInterfaceLayout {
    /// Constructs the program interface layout from validated interface facts.
    #[inline]
    #[allow(clippy::single_call_fn)]
    #[must_use]
    pub fn new(interface: &ProgramInterface, names: &StageGlobalNames) -> Self {
        let mut vertex_bindings = Vec::new();
        let mut fragment_bindings = Vec::new();
        let mut vertex_synthesized = Vec::new();
        let mut vertex_names = names.vertex.clone();
        let mut location = 0u32;

        for input in &interface.fragment_inputs {
            if let Some(output) = interface
                .vertex_outputs
                .iter()
                .find(|output| output.name == input.name)
            {
                vertex_bindings.push(StageInterfaceLayoutBinding {
                    direction: InterfaceDirection::Output,
                    name: output.name.clone(),
                    ty: output
                        .vertex_output_ty_for(input, interface.vertex_uses(input.name.as_str())),
                    location,
                });
            } else {
                let name = if vertex_names.iter().any(|name| name == &input.name) {
                    let mut index = 0u32;
                    loop {
                        let candidate = if index == 0 {
                            format!("_we_out_{}", input.name)
                        } else {
                            format!("_we_out_{}_{}", input.name, index)
                        };
                        let candidate = SmolStr::new(candidate);
                        if !vertex_names.iter().any(|name| name == &candidate) {
                            vertex_names.push(candidate.clone());
                            break candidate;
                        }
                        index += 1;
                    }
                } else {
                    vertex_names.push(input.name.clone());
                    input.name.clone()
                };
                vertex_synthesized.push(SynthesizedStageInterface {
                    stage: ShaderStageKind::Vertex,
                    direction: InterfaceDirection::Output,
                    ty: SmolStr::new(input.ty.as_str()),
                    name,
                    array_suffix: input.array_suffix.clone(),
                    location,
                    initializer: Some(StageInterfaceInitializer::Zero),
                });
            }

            fragment_bindings.push(StageInterfaceLayoutBinding {
                direction: InterfaceDirection::Input,
                name: input.name.clone(),
                ty: interface
                    .vertex_outputs
                    .iter()
                    .find(|output| output.name == input.name)
                    .and_then(|output| {
                        output.fragment_input_ty_for(
                            input,
                            interface.fragment_uses(input.name.as_str()),
                        )
                    }),
                location,
            });
            location += 1;
        }

        for output in &interface.vertex_outputs {
            if interface
                .fragment_inputs
                .iter()
                .any(|input| input.name == output.name)
            {
                continue;
            }
            vertex_bindings.push(StageInterfaceLayoutBinding {
                direction: InterfaceDirection::Output,
                name: output.name.clone(),
                ty: None,
                location,
            });
            location += 1;
        }

        Self {
            vertex: StageInterfaceLayout::new(vertex_bindings, vertex_synthesized),
            fragment: StageInterfaceLayout::new(fragment_bindings, Vec::new()),
        }
    }
}

/// Program-level descriptor layout for resources generated by codegen.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StageInterfaceBinding {
    /// Owning stage.
    stage: ShaderStageKind,
    /// Source qualifier.
    qualifier: TopLevelQualifier,
    /// Source type name.
    ty: SmolStr,
    /// Source variable name.
    name: SmolStr,
    /// Optional array suffix following the declaration name.
    array_suffix: Option<SmolStr>,
    /// Declaration span used for diagnostics.
    span: SourceSpan,
}

impl StageInterfaceBinding {
    /// Extracts top-level interface declarations from one parsed module.
    fn collect_from_module(module: &ShaderModule<'_>) -> Vec<StageInterfaceBinding> {
        module
            .items()
            .iter()
            .filter_map(|item| {
                let SyntaxItem::Declaration(declaration) = item else {
                    return None;
                };
                let suffix = declaration.array_suffix();
                let array_suffix = suffix.as_ref().map(|suffix| SmolStr::new(suffix.as_str()));
                Some(Self {
                    stage: module.stage(),
                    qualifier: declaration.qualifier()?,
                    ty: SmolStr::new(declaration.declaration_type()?.as_str()),
                    name: SmolStr::new(declaration.declaration_name()?.as_str()),
                    array_suffix,
                    span: declaration.span(),
                })
            })
            .collect()
    }

    /// Builds a query for extracting use facts for this interface.
    fn interface_use_query(&self) -> Option<InterfaceUseQuery<'_>> {
        let binding_width = LegacyTypeName::new(self.ty.as_str()).vector_width()?;
        Some(InterfaceUseQuery::new(
            self.name.as_str(),
            self.span,
            binding_width,
        ))
    }

    /// Returns the backend GLSL type spelling for this source type.
    fn glsl_ty(&self) -> &str {
        LegacyTypeName::new(self.ty.as_str()).glsl()
    }

    /// Returns true when the declarations can share one backend interface
    /// slot. Legacy HLSL allows producer/consumer declarations to disagree
    /// when the stage that declares the wider type only touches a prefix the
    /// narrower side actually provides or consumes.
    fn is_compatible_with(
        &self,
        input: &Self,
        vertex_uses: Option<&InterfaceUseFacts>,
        fragment_uses: Option<&InterfaceUseFacts>,
    ) -> bool {
        self.glsl_ty() == input.glsl_ty()
            || self
                .safe_narrowed_output_width(input, vertex_uses)
                .is_some()
            || self
                .safe_narrowed_input_width(input, fragment_uses)
                .is_some()
    }

    /// Returns the fragment width when a wider vertex output can be safely
    /// represented by the narrower fragment input.
    fn safe_narrowed_output_width(
        &self,
        input: &Self,
        vertex_uses: Option<&InterfaceUseFacts>,
    ) -> Option<u8> {
        LegacyTypeName::new(self.ty.as_str())
            .vector_width()
            .zip(LegacyTypeName::new(input.ty.as_str()).vector_width())
            .filter(|(output_width, input_width)| output_width > input_width)
            .map(|(_output_width, input_width)| input_width)
            .filter(|input_width| {
                vertex_uses.is_some_and(|uses| uses.is_prefix_compatible(*input_width))
            })
    }

    /// Returns the vertex width when a wider fragment input can be safely
    /// represented by the narrower vertex output.
    fn safe_narrowed_input_width(
        &self,
        input: &Self,
        fragment_uses: Option<&InterfaceUseFacts>,
    ) -> Option<u8> {
        LegacyTypeName::new(self.ty.as_str())
            .vector_width()
            .zip(LegacyTypeName::new(input.ty.as_str()).vector_width())
            .filter(|(output_width, input_width)| output_width < input_width)
            .map(|(output_width, _input_width)| output_width)
            .filter(|output_width| {
                fragment_uses.is_some_and(|uses| uses.is_prefix_compatible(*output_width))
            })
    }

    /// Returns a source type override for the vertex output declaration when
    /// the fragment input consumes a narrower prefix of that varying.
    fn vertex_output_ty_for(
        &self,
        input: &Self,
        vertex_uses: Option<&InterfaceUseFacts>,
    ) -> Option<SmolStr> {
        self.safe_narrowed_output_width(input, vertex_uses)
            .is_some()
            .then(|| input.ty.clone())
    }

    /// Returns a source type override for the fragment input declaration when
    /// it declares a wider type than the vertex output can provide.
    fn fragment_input_ty_for(
        &self,
        input: &Self,
        fragment_uses: Option<&InterfaceUseFacts>,
    ) -> Option<SmolStr> {
        self.safe_narrowed_input_width(input, fragment_uses)
            .is_some()
            .then(|| self.ty.clone())
    }

    /// Builds a structured pipeline-interface diagnostic at this declaration.
    fn diagnostic(&self, message: String) -> ShaderDiagnostic {
        ShaderDiagnostic::new(message)
            .with_stage(self.stage)
            .with_pass("PipelineInterface")
            .with_span(self.span)
    }
}
