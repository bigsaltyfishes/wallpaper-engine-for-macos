use std::collections::BTreeMap;

use crate::{
    ShaderMetadata, ShaderResult, ShaderTextureInfo,
    metadata::builder::MetadataBuilder,
    syntax::{AnnotationKind, ShaderDeclaration, ShaderModule, SyntaxItem},
};

pub trait ShaderModuleMetadataExt {
    /// Extracts material metadata from this parsed shader module.
    ///
    /// The scan follows the current C++ behavior: top-level metadata before the
    /// first `void main` function is considered, preprocessor conditionals are
    /// not evaluated, and declarations after `main` are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when extracted names or texture slots cannot be
    /// represented by the typed shader model.
    fn extract_metadata(&self, textures: &[ShaderTextureInfo]) -> ShaderResult<ShaderMetadata>;
}

impl ShaderModuleMetadataExt for ShaderModule<'_> {
    fn extract_metadata(&self, textures: &[ShaderTextureInfo]) -> ShaderResult<ShaderMetadata> {
        let extractor = MetadataExtractor {
            module: self,
            textures,
            builder: MetadataBuilder {
                combo_order: Vec::new(),
                combos: BTreeMap::new(),
                aliases: Vec::new(),
                default_uniforms: Vec::new(),
                default_textures: Vec::new(),
            },
            pending_declaration: None,
        };
        extractor.extract()
    }
}

/// Stateful scanner that turns parsed syntax items into material metadata.
#[derive(Debug)]
struct MetadataExtractor<'src, 'module> {
    /// Parsed module being scanned.
    module: &'module ShaderModule<'src>,
    /// Runtime texture availability used to derive texture combos.
    textures: &'module [ShaderTextureInfo],
    /// Accumulated metadata fields.
    builder: MetadataBuilder,
    /// Most recent declaration that may receive a same-line JSON annotation.
    pending_declaration: Option<&'module ShaderDeclaration<'src>>,
}

impl MetadataExtractor<'_, '_> {
    /// Scans syntax items until `void main` and returns collected metadata.
    fn extract(mut self) -> ShaderResult<ShaderMetadata> {
        for item in self.module.items() {
            match item {
                SyntaxItem::Function(function)
                    if function.return_type() == "void" && function.name() == "main" =>
                {
                    break;
                }
                SyntaxItem::Declaration(declaration) => {
                    self.pending_declaration = Some(declaration);
                }
                SyntaxItem::Annotation(annotation) => {
                    let text = annotation.text_in(self.module);
                    match annotation.kind() {
                        AnnotationKind::Combo => self.builder.handle_combo_annotation(text)?,
                        AnnotationKind::Json => {
                            if let Some(declaration) = self.pending_declaration.take()
                                && declaration.has_same_line_annotation(self.module, annotation)
                            {
                                self.builder.handle_uniform_annotation(
                                    declaration,
                                    text,
                                    self.textures,
                                )?;
                            }
                        }
                        AnnotationKind::Bracket => {}
                    }
                }
                SyntaxItem::Directive(_) | SyntaxItem::Opaque(_) | SyntaxItem::Function(_) => {
                    self.pending_declaration = None;
                }
            }
        }

        Ok(self.builder.finish())
    }
}
