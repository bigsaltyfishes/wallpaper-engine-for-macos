//! Naga-backed GLSL to SPIR-V shader compilation.

use naga::{
    back::spv,
    front::glsl,
    valid::{Capabilities, ValidationFlags, Validator},
};

use super::diagnostic::DiagnosticBuilder;
use crate::{
    CompiledShaderStage, CompiledStageArtifact, ShaderCompiler, ShaderError, ShaderResult,
    ShaderStageKind, legalize::LegalizedStageSource,
};

/// Compiler backend that lowers legalized GLSL through Naga and emits SPIR-V.
#[derive(Clone, Debug, Default)]
pub struct NagaCompiler;

impl ShaderCompiler for NagaCompiler {
    type Module = naga::Module;

    fn compile_stage(
        &self,
        stage: ShaderStageKind,
        source: &LegalizedStageSource,
    ) -> ShaderResult<CompiledStageArtifact<Self::Module>> {
        if source.stage() != stage {
            return Err(ShaderError::invalid_request(format!(
                "compiler stage {stage:?} does not match legalized source stage {:?}",
                source.stage()
            )));
        }

        let source_text = source.source();
        let source_path = match stage {
            ShaderStageKind::Vertex => "generated/vertex.glsl",
            ShaderStageKind::Fragment => "generated/fragment.glsl",
        };

        let options = glsl::Options::from(stage.into_naga());
        let mut frontend = glsl::Frontend::default();
        let module = frontend.parse(&options, source_text).map_err(|err| {
            let diagnostic = DiagnosticBuilder::new(stage, "naga glsl parse", source_path)
                .with_message(err.emit_to_string_with_path(source_text, source_path))
                .with_source(source_text)
                .with_source_location(
                    err.errors
                        .first()
                        .and_then(|error| error.location(source_text)),
                )
                .build();

            ShaderError::Compile {
                diagnostics: Box::from([diagnostic]),
            }
        })?;

        let mut validator = Validator::new(ValidationFlags::default(), Capabilities::default());
        let module_info = validator.validate(&module).map_err(|err| {
            let diagnostic = DiagnosticBuilder::new(stage, "naga validate", source_path)
                .with_message(format!("{err}"))
                .with_source(source_text)
                .with_source_location(err.location(source_text))
                .build();

            ShaderError::Compile {
                diagnostics: Box::from([diagnostic]),
            }
        })?;

        let pipeline_options = spv::PipelineOptions {
            shader_stage: stage.into_naga(),
            entry_point: "main".to_owned(),
        };
        let mut spv_options = spv::Options::default();
        spv_options
            .flags
            .remove(spv::WriterFlags::ADJUST_COORDINATE_SPACE);

        let spirv = spv::write_vec(&module, &module_info, &spv_options, Some(&pipeline_options))
            .map_err(|err| {
                let diagnostic = DiagnosticBuilder::new(stage, "naga spv write", source_path)
                    .with_message(format!(
                        "{err}\n{source_path}\n{}",
                        source_text.lines().next().unwrap_or_default()
                    ))
                    .with_source(source_text)
                    .build();

                ShaderError::Compile {
                    diagnostics: Box::from([diagnostic]),
                }
            })?;

        let compiled_stage = CompiledShaderStage::new(
            stage,
            spirv.into_boxed_slice(),
            Some(source_text.to_owned()),
            Box::from([]),
        );

        Ok(CompiledStageArtifact::new(
            compiled_stage,
            module,
            Box::from([]),
        ))
    }
}

/// Conversion into the equivalent Naga shader stage.
trait NagaShaderStageExt {
    /// Converts this stage into Naga's stage enum.
    fn into_naga(self) -> naga::ShaderStage;
}

impl NagaShaderStageExt for ShaderStageKind {
    fn into_naga(self) -> naga::ShaderStage {
        match self {
            Self::Vertex => naga::ShaderStage::Vertex,
            Self::Fragment => naga::ShaderStage::Fragment,
        }
    }
}
