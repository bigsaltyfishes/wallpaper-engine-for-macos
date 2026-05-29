use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, ShaderStageKind, SourceSpan,
    legalize::{
        InterfaceDirection, StageInterfaceInitializer, StageInterfaceLayout,
        StageInterfaceLayoutBinding, SynthesizedName, SynthesizedStageInterface, TokenSearch,
    },
    lexer::{Token, TokenKind},
    pipeline::inputs::ProgramStageInput,
    syntax::{ShaderModule, SyntaxItem, TopLevelQualifier},
};

/// Program-level vertex/fragment interface summary.
#[derive(Debug, Default)]
pub(super) struct ProgramInterface<'src> {
    /// Vertex stage outputs in the location order the legalizer will emit.
    vertex_outputs: Vec<StageInterfaceBinding<'src>>,
    /// Fragment stage inputs in the location order the legalizer will emit.
    fragment_inputs: Vec<StageInterfaceBinding<'src>>,
    /// Vertex-stage usage summaries keyed by output varying name.
    vertex_output_uses: BTreeMap<&'src str, InterfaceUses>,
    /// Fragment-stage usage summaries keyed by input varying name.
    fragment_input_uses: BTreeMap<&'src str, InterfaceUses>,
}

impl<'src> From<&[ProgramStageInput<'src>]> for ProgramInterface<'src> {
    /// Extracts cross-stage interface declarations from parsed stages.
    fn from(stages: &[ProgramStageInput<'src>]) -> Self {
        let mut interface = Self::default();
        for stage in stages {
            match stage.stage.kind() {
                ShaderStageKind::Vertex => {
                    let outputs = StageInterfaceBinding::from_module(&stage.module)
                        .into_iter()
                        .filter(|binding| {
                            matches!(
                                binding.qualifier,
                                TopLevelQualifier::Varying | TopLevelQualifier::Out
                            ) && binding.name != "_ww_sv_position"
                        })
                        .collect::<Vec<_>>();
                    for output in &outputs {
                        let uses = output.uses_in_module(&stage.module);
                        let _previous = interface.vertex_output_uses.insert(output.name, uses);
                    }
                    interface.vertex_outputs.extend(outputs);
                }
                ShaderStageKind::Fragment => {
                    let inputs = StageInterfaceBinding::from_module(&stage.module)
                        .into_iter()
                        .filter(|binding| {
                            matches!(
                                binding.qualifier,
                                TopLevelQualifier::Varying | TopLevelQualifier::In
                            ) && binding.name != "_ww_sv_position"
                        })
                        .collect::<Vec<_>>();
                    for input in &inputs {
                        let uses = input.uses_in_module(&stage.module);
                        let _previous = interface.fragment_input_uses.insert(input.name, uses);
                    }
                    interface.fragment_inputs.extend(inputs);
                }
            }
        }
        interface
    }
}

impl<'src> ProgramInterface<'src> {
    /// Validates and builds a program-level interface layout while avoiding
    /// synthesized declaration name collisions with stage globals.
    pub(super) fn validate_with_names(
        &self,
        names: &StageGlobalNames<'src>,
    ) -> ShaderResult<ProgramInterfaceLayout<'src>> {
        if let Some(diagnostic) = self.first_duplicate_diagnostic() {
            return Err(Self::error(diagnostic));
        }
        if let Some(diagnostic) = self.first_incompatible_type_diagnostic() {
            return Err(Self::error(diagnostic));
        }
        Ok(ProgramInterfaceLayout::from_interface_and_names(
            self, names,
        ))
    }

    /// Builds a legalization error for program-interface diagnostics.
    fn error(diagnostic: ShaderDiagnostic) -> ShaderError {
        ShaderError::Legalize {
            diagnostics: Box::from([diagnostic]),
        }
    }

    /// Finds duplicate cross-stage declarations inside a single stage.
    fn first_duplicate_diagnostic(&self) -> Option<ShaderDiagnostic> {
        Self::first_duplicate(&self.vertex_outputs)
            .or_else(|| Self::first_duplicate(&self.fragment_inputs))
    }

    /// Finds the first duplicate binding in declaration order.
    fn first_duplicate(bindings: &[StageInterfaceBinding<'_>]) -> Option<ShaderDiagnostic> {
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
            let vertex_uses = self.vertex_output_uses.get(output.name);
            let fragment_uses = self.fragment_input_uses.get(input.name);
            (!output.is_compatible_with(*input, vertex_uses, fragment_uses)).then(|| {
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
}

/// Stage-local global names that synthesized declarations must not reuse.
#[derive(Debug, Default)]
pub(super) struct StageGlobalNames<'src> {
    /// Vertex-stage top-level declaration names.
    vertex: BTreeSet<&'src str>,
}

impl<'src> From<&[ProgramStageInput<'src>]> for StageGlobalNames<'src> {
    /// Extracts vertex globals from parsed top-level declarations.
    fn from(stages: &[ProgramStageInput<'src>]) -> Self {
        let mut names = Self::default();
        for stage in stages {
            if stage.stage.kind() != ShaderStageKind::Vertex {
                continue;
            }
            for item in stage.module.items() {
                let SyntaxItem::Declaration(declaration) = item else {
                    continue;
                };
                if let Some(name) = (StageGlobalDeclaration {
                    module: &stage.module,
                    declaration,
                })
                .name()
                {
                    let _inserted = names.vertex.insert(name);
                }
            }
        }
        names
    }
}

/// Vertex-stage global declaration fact used for generated-name collision
/// checks.
#[derive(Clone, Copy)]
struct StageGlobalDeclaration<'module, 'src> {
    /// Parsed module containing the declaration.
    module: &'module ShaderModule<'src>,
    /// Source declaration.
    declaration: &'module crate::syntax::ShaderDeclaration<'src>,
}

impl<'src> StageGlobalDeclaration<'_, 'src> {
    /// Returns the global name, preferring syntax facts and falling back only
    /// when the lightweight parser did not classify an unqualified declaration.
    fn name(self) -> Option<&'src str> {
        if let Some(name) = self.declaration.declaration_name() {
            return Some(name.as_str());
        }
        StageGlobalDeclarationTokens {
            tokens: self.module.tokens(),
            span: self.declaration.span(),
        }
        .name()
    }
}

/// Token-backed global declaration fact for unqualified declarations that the
/// syntax parser has not yet classified fully.
#[derive(Clone, Copy)]
struct StageGlobalDeclarationTokens<'tokens, 'src> {
    /// Module tokens.
    tokens: &'tokens [Token<'src>],
    /// Declaration source span.
    span: SourceSpan,
}

impl<'src> StageGlobalDeclarationTokens<'_, 'src> {
    /// Returns the first declarator name from this top-level declaration.
    fn name(self) -> Option<&'src str> {
        let first = self
            .tokens
            .iter()
            .position(|token| token.span.start() >= self.span.start())?;
        let semicolon = self
            .tokens
            .iter()
            .enumerate()
            .skip(first)
            .find_map(|(index, token)| {
                (token.span.end() <= self.span.end() && matches!(token.kind, TokenKind::Semicolon))
                    .then_some(index)
            })?;
        let mut identifiers = self
            .tokens
            .iter()
            .take(semicolon)
            .skip(first)
            .filter_map(|token| match token.kind {
                TokenKind::Identifier(text) if !token.kind.is_declaration_modifier() => Some(text),
                _ => None,
            });
        let _type_name = identifiers.next()?;
        identifiers.next()
    }
}

/// Program-level location layout for cross-stage interfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProgramInterfaceLayout<'src> {
    /// Vertex stage layout.
    vertex: StageInterfaceLayout<'src>,
    /// Fragment stage layout.
    fragment: StageInterfaceLayout<'src>,
}

impl<'src> From<&ProgramInterface<'src>> for ProgramInterfaceLayout<'src> {
    /// Builds matching stage layouts from a validated program interface.
    fn from(interface: &ProgramInterface<'src>) -> Self {
        Self::from_interface_and_names(interface, &StageGlobalNames::default())
    }
}

impl<'src> ProgramInterfaceLayout<'src> {
    /// Builds matching stage layouts from a validated program interface.
    fn from_interface_and_names(
        interface: &ProgramInterface<'src>,
        names: &StageGlobalNames<'src>,
    ) -> Self {
        let mut vertex_bindings = Vec::new();
        let mut fragment_bindings = Vec::new();
        let mut vertex_synthesized = Vec::new();
        let mut vertex_names = names
            .vertex
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>();
        let mut location = 0u32;

        for input in &interface.fragment_inputs {
            if let Some(output) = interface
                .vertex_outputs
                .iter()
                .find(|output| output.name == input.name)
            {
                vertex_bindings.push(StageInterfaceLayoutBinding {
                    direction: InterfaceDirection::Output,
                    name: output.name,
                    ty: output.vertex_output_ty_for(
                        *input,
                        interface.vertex_output_uses.get(output.name),
                    ),
                    location,
                });
            } else {
                let name = if vertex_names.insert(input.name.to_owned()) {
                    SynthesizedName::Source(input.name)
                } else {
                    let mut index = 0u32;
                    loop {
                        let candidate = if index == 0 {
                            format!("_we_out_{}", input.name)
                        } else {
                            format!("_we_out_{}_{}", input.name, index)
                        };
                        if vertex_names.insert(candidate.clone()) {
                            break SynthesizedName::Generated(candidate);
                        }
                        index += 1;
                    }
                };
                vertex_synthesized.push(SynthesizedStageInterface {
                    stage: ShaderStageKind::Vertex,
                    direction: InterfaceDirection::Output,
                    ty: input.ty,
                    name,
                    array_suffix: input.array_suffix,
                    location,
                    initializer: Some(StageInterfaceInitializer::Zero),
                });
            }

            fragment_bindings.push(StageInterfaceLayoutBinding {
                direction: InterfaceDirection::Input,
                name: input.name,
                ty: interface
                    .vertex_outputs
                    .iter()
                    .find(|output| output.name == input.name)
                    .and_then(|output| {
                        output.fragment_input_ty_for(
                            *input,
                            interface.fragment_input_uses.get(input.name),
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
                name: output.name,
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

    /// Returns the stage-local layout for `stage`.
    pub(super) fn layout_for_stage(&self, stage: ShaderStageKind) -> StageInterfaceLayout<'src> {
        match stage {
            ShaderStageKind::Vertex => self.vertex.clone(),
            ShaderStageKind::Fragment => self.fragment.clone(),
        }
    }
}

/// Program-level descriptor layout for resources generated by legalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StageInterfaceBinding<'src> {
    /// Owning stage.
    stage: ShaderStageKind,
    /// Source qualifier.
    qualifier: TopLevelQualifier,
    /// Source type name.
    ty: &'src str,
    /// Source variable name.
    name: &'src str,
    /// Optional array suffix following the declaration name.
    array_suffix: Option<&'src str>,
    /// Declaration span used for diagnostics.
    span: SourceSpan,
}

impl<'src> StageInterfaceBinding<'src> {
    /// Extracts top-level interface declarations from one parsed module.
    fn from_module(module: &ShaderModule<'src>) -> Vec<StageInterfaceBinding<'src>> {
        module
            .items()
            .iter()
            .filter_map(|item| {
                let SyntaxItem::Declaration(declaration) = item else {
                    return None;
                };
                let suffix = declaration.array_suffix();
                let array_suffix = suffix.as_ref().map(|suffix| suffix.as_str());
                Some(Self {
                    stage: module.stage(),
                    qualifier: declaration.qualifier()?,
                    ty: declaration.declaration_type()?.as_str(),
                    name: declaration.declaration_name()?.as_str(),
                    array_suffix,
                    span: declaration.span(),
                })
            })
            .collect()
    }

    /// Returns the emitted GLSL type spelling used by the legalizer.
    const fn glsl_ty(self) -> &'src str {
        match self.ty.as_bytes() {
            b"float1" => "float",
            b"float2" => "vec2",
            b"float3" => "vec3",
            b"float4" => "vec4",
            _ => self.ty,
        }
    }

    /// Extracts token-level usage of this declaration inside its stage body.
    fn uses_in_module(self, module: &ShaderModule<'src>) -> InterfaceUses {
        let Some(binding_width) = vector_width(self.glsl_ty()) else {
            return InterfaceUses::default();
        };
        let tokens = module.tokens();
        let mut uses = InterfaceUses::default();
        for (index, token) in tokens.iter().enumerate() {
            if token.span.start() < self.span.end() && token.span.end() > self.span.start() {
                continue;
            }
            if !matches!(token.kind, TokenKind::Identifier(name) if name == self.name) {
                continue;
            }
            let reference = VaryingReference {
                tokens,
                index,
                binding_width,
            }
            .classify();
            uses.references.push(reference);
        }
        uses
    }

    /// Returns true when the declarations can share one backend interface
    /// slot. Legacy HLSL allows producer/consumer declarations to disagree
    /// when the stage that declares the wider type only touches a prefix the
    /// narrower side actually provides or consumes.
    fn is_compatible_with(
        self,
        input: Self,
        vertex_uses: Option<&InterfaceUses>,
        fragment_uses: Option<&InterfaceUses>,
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
        self,
        input: Self,
        vertex_uses: Option<&InterfaceUses>,
    ) -> Option<u8> {
        vector_width(self.glsl_ty())
            .zip(vector_width(input.glsl_ty()))
            .filter(|(output_width, input_width)| output_width > input_width)
            .map(|(_output_width, input_width)| input_width)
            .filter(|input_width| {
                vertex_uses.is_some_and(|uses| uses.is_prefix_compatible(*input_width))
            })
    }

    /// Returns the vertex width when a wider fragment input can be safely
    /// represented by the narrower vertex output.
    fn safe_narrowed_input_width(
        self,
        input: Self,
        fragment_uses: Option<&InterfaceUses>,
    ) -> Option<u8> {
        vector_width(self.glsl_ty())
            .zip(vector_width(input.glsl_ty()))
            .filter(|(output_width, input_width)| output_width < input_width)
            .map(|(output_width, _input_width)| output_width)
            .filter(|output_width| {
                fragment_uses.is_some_and(|uses| uses.is_prefix_compatible(*output_width))
            })
    }

    /// Returns a source type override for the vertex output declaration when
    /// the fragment input consumes a narrower prefix of that varying.
    fn vertex_output_ty_for(
        self,
        input: Self,
        vertex_uses: Option<&InterfaceUses>,
    ) -> Option<&'src str> {
        self.safe_narrowed_output_width(input, vertex_uses)
            .is_some()
            .then_some(input.ty)
    }

    /// Returns a source type override for the fragment input declaration when
    /// it declares a wider type than the vertex output can provide.
    fn fragment_input_ty_for(
        self,
        input: Self,
        fragment_uses: Option<&InterfaceUses>,
    ) -> Option<&'src str> {
        self.safe_narrowed_input_width(input, fragment_uses)
            .is_some()
            .then_some(self.ty)
    }

    /// Builds a structured pipeline-interface diagnostic at this declaration.
    fn diagnostic(self, message: String) -> ShaderDiagnostic {
        ShaderDiagnostic::new(message)
            .with_stage(self.stage)
            .with_pass("PipelineInterface")
            .with_span(self.span)
    }
}

/// Stage-local component usage for one cross-stage varying.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct InterfaceUses {
    /// References found outside the top-level declaration.
    references: Vec<InterfaceReference>,
}

impl InterfaceUses {
    /// Returns true when all stage references stay within `width`.
    fn is_prefix_compatible(&self, width: u8) -> bool {
        self.references.iter().all(|reference| match reference {
            InterfaceReference::Swizzle { required_width } => *required_width <= width,
            InterfaceReference::PlainAssignment => true,
            InterfaceReference::PlainRead => false,
        })
    }
}

/// One reference to a cross-stage varying.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InterfaceReference {
    /// Direct component swizzle reference.
    Swizzle {
        /// Minimum prefix width needed by the swizzle.
        required_width: u8,
    },
    /// Whole-variable assignment that the legalizer can narrow consistently.
    PlainAssignment,
    /// Whole-variable read or unsupported reference that is unsafe to narrow.
    PlainRead,
}

/// One token-level reference to a cross-stage varying.
#[derive(Clone, Copy)]
struct VaryingReference<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [Token<'src>],
    /// Index of the varying identifier token.
    index: usize,
    /// Declared interface component width.
    binding_width: u8,
}

impl VaryingReference<'_, '_> {
    /// Classifies this varying reference for prefix-compatibility checks.
    fn classify(self) -> InterfaceReference {
        let search = TokenSearch::new(self.tokens);
        let Some(dot) = search.next_non_comment(self.index + 1) else {
            return self.plain_reference();
        };
        if !matches!(self.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return self.plain_reference();
        }
        let Some(field) = search.next_non_comment(dot + 1) else {
            return InterfaceReference::PlainRead;
        };
        let TokenKind::Identifier(field) = self.tokens[field].kind else {
            return InterfaceReference::PlainRead;
        };
        InterfaceReference::Swizzle {
            required_width: field
                .bytes()
                .try_fold(0, |width, component| {
                    match component {
                        b'x' | b'r' | b's' => Some(1),
                        b'y' | b'g' | b't' => Some(2),
                        b'z' | b'b' | b'p' => Some(3),
                        b'w' | b'a' | b'q' => Some(4),
                        _ => None,
                    }
                    .map(|index| width.max(index))
                })
                .unwrap_or(self.binding_width),
        }
    }

    /// Classifies a whole-variable reference.
    fn plain_reference(self) -> InterfaceReference {
        let search = TokenSearch::new(self.tokens);
        if search
            .next_non_comment(self.index + 1)
            .is_some_and(|next| matches!(self.tokens[next].kind, TokenKind::Punctuation('=')))
        {
            InterfaceReference::PlainAssignment
        } else {
            InterfaceReference::PlainRead
        }
    }
}

/// Extracts source array suffixes from top-level interface declarations.
const fn vector_width(ty: &str) -> Option<u8> {
    match ty.as_bytes() {
        b"float" | b"float1" => Some(1),
        b"vec2" | b"float2" => Some(2),
        b"vec3" | b"float3" => Some(3),
        b"vec4" | b"float4" => Some(4),
        _ => None,
    }
}
