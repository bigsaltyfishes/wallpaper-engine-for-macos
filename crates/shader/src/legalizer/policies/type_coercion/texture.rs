use super::{
    Fixup, FunctionCallIndex, PolicyContext, SourceSpan, TextureSamplingCall, Token, TokenKind,
    TokenSearch,
    types::{SourcePointExt, VectorWidth},
};

/// Vector declarations initialized from wider texture samples.
pub(super) struct TextureVectorInitializers {
    /// Narrowing swizzle insertions in source order.
    pub(super) items: Vec<TextureVectorInitializer>,
}
impl From<&mut PolicyContext<'_, '_, '_>> for TextureVectorInitializers {
    fn from(context: &mut PolicyContext<'_, '_, '_>) -> Self {
        let state = context.context();
        let tokens = state.module.tokens();
        let calls = FunctionCallIndex::new(tokens);
        let mut items = Vec::new();
        for call in calls.iter() {
            let Some(_texture_call) = TextureSamplingCall::from_call(call, &state.declarations)
            else {
                continue;
            };
            if call.has_trailing_swizzle() {
                continue;
            }
            let Ok(width) = TextureVectorInitializerWidth::try_from(TextureInitializerTarget {
                tokens,
                call_name: call.name_index,
            }) else {
                continue;
            };
            items.push(TextureVectorInitializer {
                call_span: call.span(),
                width,
            });
        }
        Self { items }
    }
}
impl IntoIterator for TextureVectorInitializers {
    type Item = TextureVectorInitializer;
    type IntoIter = std::vec::IntoIter<TextureVectorInitializer>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Target declaration before a texture initializer call.
pub(super) struct TextureInitializerTarget<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Function-name token index of the texture call.
    pub(super) call_name: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Target vector width for a texture initializer.
pub(super) struct TextureVectorInitializerWidth {
    /// Vector width being initialized.
    pub(super) width: VectorWidth,
}
impl TryFrom<TextureInitializerTarget<'_, '_>> for TextureVectorInitializerWidth {
    type Error = ();

    fn try_from(target: TextureInitializerTarget<'_, '_>) -> Result<Self, Self::Error> {
        let search = TokenSearch::new(target.tokens);
        let equals = search.previous_non_comment(target.call_name).ok_or(())?;
        let tokens = target.tokens;
        if !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
            return Err(());
        }
        let name = search.previous_non_comment(equals).ok_or(())?;
        if !matches!(tokens[name].kind, TokenKind::Identifier(_)) {
            return Err(());
        }
        let ty = search.previous_non_comment(name).ok_or(())?;
        let TokenKind::Identifier(type_name) = tokens[ty].kind else {
            return Err(());
        };
        let width = VectorWidth::from_constructor(type_name).ok_or(())?;
        width
            .narrow_swizzle()
            .map(|_swizzle| Self { width })
            .ok_or(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Texture initializer that needs a vector-width swizzle.
pub(super) struct TextureVectorInitializer {
    /// Full texture sampling call span.
    pub(super) call_span: SourceSpan,
    /// Target vector width.
    pub(super) width: TextureVectorInitializerWidth,
}
impl TextureVectorInitializer {
    /// Emits insertions around the texture call and a target-width swizzle.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        let Some(swizzle) = self.width.width.narrow_swizzle() else {
            return;
        };
        context
            .context()
            .fixups
            .push(Fixup::insert(self.call_span.start_point(), "(".to_owned()));
        context.context().fixups.push(Fixup::insert(
            self.call_span.end_point(),
            format!("){swizzle}"),
        ));
    }
}
