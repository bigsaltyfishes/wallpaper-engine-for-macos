use crate::{PropertyValue, ShaderError, ShaderResult, TextureSlot};

/// Borrowed, minimally validated JSON object slice.
struct JsonObject<'src> {
    /// Object text including braces.
    source: &'src str,
}

impl<'src> JsonObject<'src> {
    /// Returns a decoded string value by key.
    fn string(&self, key: &str) -> Option<&'src str> {
        self.value_for_key(key)
            .and_then(|value| value.strip_prefix('"'))
            .and_then(|value| {
                JsonStringSlice::new(value)
                    .read()
                    .ok()
                    .flatten()
                    .map(|(text, _)| text)
            })
    }

    /// Returns a numeric value slice by key.
    fn number_text(&self, key: &str) -> Option<&'src str> {
        self.value_for_key(key)
            .and_then(|value| JsonNumberSlice::new(value).read())
    }

    /// Returns a typed property value by key.
    fn property_value(&self, key: &str) -> ShaderResult<Option<PropertyValue>> {
        let Some(value) = self.value_for_key(key) else {
            return Ok(None);
        };
        if let Some(number) = JsonNumberSlice::new(value).read() {
            return Ok(Some(PropertyValue::Number(number.parse_metadata_f32()?)));
        }
        if let Some(text) = value.strip_prefix('"') {
            let Some((decoded, _)) = JsonStringSlice::new(text).read()? else {
                return Ok(None);
            };
            return (AnnotationDefault { source: decoded })
                .into_property_value()
                .map(Some);
        }
        if value.starts_with("true") {
            return Ok(Some(PropertyValue::Bool(true)));
        }
        if value.starts_with("false") {
            return Ok(Some(PropertyValue::Bool(false)));
        }
        Ok(None)
    }

    /// Returns a raw value slice by object key.
    fn value_for_key(&self, key: &str) -> Option<&'src str> {
        let body = self.source.get(1..self.source.len().saturating_sub(1))?;
        let mut cursor = JsonObjectCursor { remaining: body };
        while let Ok(Some(entry)) = cursor.next_entry() {
            if entry.key() == key {
                return Some(entry.value().trim_start());
            }
        }
        None
    }

    /// Returns a balanced array slice by key.
    fn array_for_key(&self, key: &str) -> ShaderResult<Option<&'src str>> {
        let Some(value) = self.value_for_key(key) else {
            return Ok(None);
        };
        let value = value.trim_start();
        if !value.starts_with('[') {
            return Ok(None);
        }
        JsonDelimitedSlice::new(value, b'[', b']').matching()
    }
}

/// Typed annotation payload parsed from a Wallpaper Engine JSON annotation.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParsedAnnotation<'src> {
    /// Optional material alias target.
    material: Option<&'src str>,
    /// Optional default value payload.
    default: Option<AnnotationDefaultValue<'src>>,
    /// Optional numeric combo default exactly as written.
    combo_default: Option<&'src str>,
    /// Optional top-level combo name.
    combo: Option<&'src str>,
    /// Component-level combo names in source order.
    component_combos: Vec<Option<&'src str>>,
}

impl<'src> ParsedAnnotation<'src> {
    /// Parses the first JSON object from annotation text.
    pub(super) fn from_annotation_text(text: &'src str) -> ShaderResult<Option<Self>> {
        let Some(json) = text.find('{').and_then(|index| text.get(index..)) else {
            return Ok(None);
        };
        let trimmed = json.trim();
        if trimmed.is_empty() || !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
            return Ok(None);
        }
        if JsonDelimitedSlice::new(trimmed, b'{', b'}')
            .validate_balanced()
            .is_err()
        {
            return Ok(None);
        }
        JsonObject { source: trimmed }
            .into_annotation_payload()
            .map(Some)
    }

    /// Returns the parsed material alias target.
    pub(super) const fn material(&self) -> Option<&'src str> {
        self.material
    }

    /// Returns the parsed default value payload.
    pub(super) fn default(&self) -> Option<&AnnotationDefaultValue<'src>> {
        self.default.as_ref()
    }

    /// Returns the parsed top-level combo name.
    pub(super) const fn combo(&self) -> Option<&'src str> {
        self.combo
    }

    /// Iterates component combo names in source order.
    pub(super) fn component_combos(&self) -> impl Iterator<Item = Option<&'src str>> + '_ {
        self.component_combos.iter().copied()
    }

    /// Returns the default value formatted for a combo annotation.
    pub(super) const fn combo_default_value(&self) -> Option<&'src str> {
        self.combo_default
    }
}

/// Typed default value parsed from annotation JSON.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AnnotationDefaultValue<'src> {
    /// Decoded string default, interpreted by the consuming uniform kind.
    String(&'src str),
    /// Scalar/vector/bool uniform default parsed from a non-string JSON value.
    Property(PropertyValue),
}

/// Numeric metadata parsing for borrowed JSON number text.
trait MetadataNumberExt {
    /// Parses a metadata number into an `f32`.
    fn parse_metadata_f32(self) -> ShaderResult<f32>;
}

impl MetadataNumberExt for &str {
    fn parse_metadata_f32(self) -> ShaderResult<f32> {
        self.parse::<f32>()
            .map_err(|_| ShaderError::invalid_request("metadata number is invalid"))
    }
}

/// Uniform name that may encode a Wallpaper Engine texture slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TextureUniformName<'src> {
    /// Uniform identifier text.
    pub(super) source: &'src str,
}

impl TextureUniformName<'_> {
    /// Returns the encoded texture slot when this is a texture uniform name.
    pub(super) fn slot(self) -> ShaderResult<Option<TextureSlot>> {
        let Some(suffix) = self.source.strip_prefix("g_Texture") else {
            return Ok(None);
        };
        let Ok(slot) = suffix.parse::<u8>() else {
            return Ok(None);
        };
        TextureSlot::new(slot).map(Some)
    }
}

impl<'src> JsonObject<'src> {
    /// Converts this raw JSON object into a typed annotation payload.
    fn into_annotation_payload(self) -> ShaderResult<ParsedAnnotation<'src>> {
        Ok(ParsedAnnotation {
            material: self.string("material"),
            default: self.default_value()?,
            combo_default: self.number_text("default"),
            combo: self.string("combo"),
            component_combos: self.component_combo_values()?,
        })
    }

    /// Returns the typed default value payload by preserving both texture-path
    /// and property-value interpretations at the parsing boundary.
    fn default_value(&self) -> ShaderResult<Option<AnnotationDefaultValue<'src>>> {
        if let Some(source) = self.string("default") {
            return Ok(Some(AnnotationDefaultValue::String(source)));
        }
        self.property_value("default")
            .map(|value| value.map(AnnotationDefaultValue::Property))
    }

    /// Returns all component combo payloads in source order.
    fn component_combo_values(&self) -> ShaderResult<Vec<Option<&'src str>>> {
        let Some(components) = self.array_for_key("components")? else {
            return Ok(Vec::new());
        };

        let mut cursor = JsonArrayCursor {
            remaining: components,
        };
        let mut combos = Vec::new();
        while let Some(element) = cursor.next_object()? {
            combos.push(element.string("combo"));
        }
        Ok(combos)
    }
}

/// String default annotation that may contain scalar/vector text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnnotationDefault<'src> {
    /// Decoded default string content.
    source: &'src str,
}

impl AnnotationDefault<'_> {
    /// Converts the annotation text into the closest property value.
    fn into_property_value(self) -> ShaderResult<PropertyValue> {
        let mut values = Vec::new();
        for part in self
            .source
            .split(|character: char| character.is_ascii_whitespace() || character == ',')
        {
            if part.is_empty() {
                continue;
            }
            values.push(part.parse_metadata_f32()?);
        }

        match values.as_slice() {
            [one] => Ok(PropertyValue::Number(*one)),
            [x, y, z] => Ok(PropertyValue::Vec3([*x, *y, *z])),
            _ => Ok(PropertyValue::String(self.source.to_owned())),
        }
    }
}

/// Borrowed JSON object entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonObjectEntry<'src> {
    /// Decoded entry key.
    key: &'src str,
    /// Raw entry value slice.
    value: &'src str,
}

impl<'src> JsonObjectEntry<'src> {
    /// Returns the decoded key.
    const fn key(self) -> &'src str {
        self.key
    }

    /// Returns the raw value slice.
    const fn value(self) -> &'src str {
        self.value
    }
}

/// Cursor over comma-separated JSON object entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonObjectCursor<'src> {
    /// Unconsumed object body text.
    remaining: &'src str,
}

impl<'src> JsonObjectCursor<'src> {
    /// Returns the next parseable object entry.
    fn next_entry(&mut self) -> ShaderResult<Option<JsonObjectEntry<'src>>> {
        let cursor = self.remaining.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == ','
        });
        if cursor.is_empty() {
            self.remaining = cursor;
            return Ok(None);
        }

        let Some(rest) = cursor.strip_prefix('"') else {
            self.remaining = cursor;
            return Ok(None);
        };
        let Some((key, after_key)) = JsonStringSlice::new(rest).read()? else {
            self.remaining = cursor;
            return Ok(None);
        };
        let after_colon = after_key.trim_start();
        let Some(value_start) = after_colon.strip_prefix(':') else {
            self.remaining = cursor;
            return Ok(None);
        };
        let value_start = value_start.trim_start();
        let value_end = JsonValueSlice {
            source: value_start,
        }
        .end_offset()?;
        let Some(value) = value_start.get(..value_end) else {
            self.remaining = cursor;
            return Ok(None);
        };

        self.remaining = value_start.get(value_end..).unwrap_or("");
        Ok(Some(JsonObjectEntry { key, value }))
    }
}

/// Cursor over JSON array elements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonArrayCursor<'src> {
    /// Unconsumed array text.
    remaining: &'src str,
}

impl<'src> JsonArrayCursor<'src> {
    /// Returns the next object element from the array.
    fn next_object(&mut self) -> ShaderResult<Option<JsonObject<'src>>> {
        let cursor = self.remaining.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == ',' || character == '['
        });
        if cursor.is_empty() || cursor.starts_with(']') {
            self.remaining = cursor;
            return Ok(None);
        }
        let Some(object) = JsonDelimitedSlice::new(cursor, b'{', b'}').matching()? else {
            self.remaining = cursor;
            return Ok(None);
        };
        self.remaining = cursor.get(object.len()..).unwrap_or("");
        Ok(Some(JsonObject { source: object }))
    }
}

/// Balanced JSON delimiter parser for object and array slices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonDelimitedSlice<'src> {
    /// Source slice starting at an opening delimiter.
    source: &'src str,
    /// Opening delimiter byte.
    open: u8,
    /// Closing delimiter byte.
    close: u8,
}

impl<'src> JsonDelimitedSlice<'src> {
    /// Creates a delimiter parser for a source slice.
    const fn new(source: &'src str, open: u8, close: u8) -> Self {
        Self {
            source,
            open,
            close,
        }
    }

    /// Returns the shortest balanced delimited prefix.
    fn matching(self) -> ShaderResult<Option<&'src str>> {
        let bytes = self.source.as_bytes();
        if bytes.first().copied() != Some(self.open) {
            return Ok(None);
        }

        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (index, byte) in bytes.iter().copied().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }

            if byte == b'"' {
                in_string = true;
                continue;
            }

            if byte == self.open {
                depth += 1;
            } else if byte == self.close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = index + 1;
                    return self
                        .source
                        .get(..end)
                        .map(Some)
                        .ok_or_else(|| ShaderError::invalid_request("metadata span is invalid"));
                }
            }
        }

        Err(ShaderError::invalid_request(
            "metadata object is unbalanced",
        ))
    }

    /// Returns the byte length of the balanced delimited prefix.
    fn end_offset(self) -> ShaderResult<usize> {
        self.matching().map(|value| value.map_or(0, str::len))
    }

    /// Ensures this entire slice is one balanced delimited value.
    fn validate_balanced(self) -> ShaderResult<()> {
        let Some(matched) = self.matching()? else {
            return Err(ShaderError::invalid_request("metadata object is invalid"));
        };
        if matched.len() != self.source.len() {
            return Err(ShaderError::invalid_request("metadata object is invalid"));
        }
        Ok(())
    }
}

/// Raw JSON value slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonValueSlice<'src> {
    /// Source text beginning at a JSON value.
    source: &'src str,
}

impl JsonValueSlice<'_> {
    /// Returns the byte length of this JSON value.
    fn end_offset(self) -> ShaderResult<usize> {
        match self.source.as_bytes().first().copied() {
            Some(b'"') => JsonStringSlice::new(self.source).end_offset(),
            Some(b'{') => JsonDelimitedSlice::new(self.source, b'{', b'}').end_offset(),
            Some(b'[') => JsonDelimitedSlice::new(self.source, b'[', b']').end_offset(),
            Some(_) => Ok(self
                .source
                .find(',')
                .or_else(|| self.source.find('}'))
                .unwrap_or(self.source.len())),
            None => Ok(0),
        }
    }
}

/// JSON string contents after the opening quote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonStringSlice<'src> {
    /// Source text after an opening quote, or including one for offset reads.
    source: &'src str,
}

impl<'src> JsonStringSlice<'src> {
    /// Creates a string parser for source text.
    const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Reads until an unescaped closing quote.
    fn read(self) -> ShaderResult<Option<(&'src str, &'src str)>> {
        let mut escaped = false;
        for (index, character) in self.source.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '"' => {
                    let value = self.source.get(..index).ok_or_else(|| {
                        ShaderError::invalid_request("metadata string span is invalid")
                    })?;
                    let rest = self.source.get(index + 1..).ok_or_else(|| {
                        ShaderError::invalid_request("metadata string span is invalid")
                    })?;
                    return Ok(Some((value, rest)));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// Returns the byte length of a quoted string value.
    fn end_offset(self) -> ShaderResult<usize> {
        let Some(rest) = self.source.strip_prefix('"') else {
            return Ok(0);
        };
        let Some((_, after)) = JsonStringSlice::new(rest).read()? else {
            return Ok(0);
        };
        Ok(self.source.len().saturating_sub(after.len()))
    }
}

/// JSON number prefix parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JsonNumberSlice<'src> {
    /// Source text beginning at a potential number.
    source: &'src str,
}

impl<'src> JsonNumberSlice<'src> {
    /// Creates a number parser for source text.
    const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Returns the numeric prefix, if present.
    fn read(self) -> Option<&'src str> {
        let end = self
            .source
            .char_indices()
            .take_while(|(_, character)| {
                character.is_ascii_digit() || matches!(character, '-' | '+' | '.' | 'e' | 'E')
            })
            .map(|(index, character)| index + character.len_utf8())
            .last()?;

        self.source.get(..end).filter(|value| !value.is_empty())
    }
}
