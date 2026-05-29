//! Interface-use facts extracted from module tokens.

use smol_str::SmolStr;

use crate::SourceSpan;

/// Query for collecting usage facts for one stage interface declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceUseQuery<'src> {
    /// Source variable name.
    name: &'src str,
    /// Declaration span to exclude from reference scanning.
    declaration_span: SourceSpan,
    /// Declared interface component width.
    binding_width: u8,
}

impl<'src> InterfaceUseQuery<'src> {
    /// Creates a stage interface usage query.
    #[must_use]
    pub const fn new(name: &'src str, declaration_span: SourceSpan, binding_width: u8) -> Self {
        Self {
            name,
            declaration_span,
            binding_width,
        }
    }

    /// Returns the queried interface name.
    #[must_use]
    pub const fn name(self) -> &'src str {
        self.name
    }

    /// Returns the declaration span that should be excluded from the scan.
    #[must_use]
    pub const fn declaration_span(self) -> SourceSpan {
        self.declaration_span
    }

    /// Returns the declared component width.
    #[must_use]
    pub const fn binding_width(self) -> u8 {
        self.binding_width
    }
}

/// Stage-local component usage for one cross-stage interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceUseFacts {
    /// Source variable name.
    pub name: SmolStr,
    /// Declaration span excluded from the reference scan.
    pub declaration_span: SourceSpan,
    /// References found outside the top-level declaration.
    pub references: Vec<InterfaceReference>,
}

impl InterfaceUseFacts {
    /// Returns the interface name these usage facts describe.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the declaration span these usage facts exclude.
    #[must_use]
    pub const fn declaration_span(&self) -> SourceSpan {
        self.declaration_span
    }

    /// Returns true when all stage references stay within `width`.
    #[must_use]
    pub fn is_prefix_compatible(&self, width: u8) -> bool {
        self.references.iter().all(|reference| match reference {
            InterfaceReference::Swizzle { required_width } => *required_width <= width,
            InterfaceReference::PlainAssignment => true,
            InterfaceReference::PlainRead => false,
        })
    }
}

/// One reference to a cross-stage interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceReference {
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
