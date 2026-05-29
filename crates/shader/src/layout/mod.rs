//! Shader interface locations and descriptor bindings used during legalization.

use crate::{BindingIndex, BindingSet, LocationIndex, ShaderResult};

/// Monotonic allocator for stage interface locations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocationAllocator {
    /// Next unassigned numeric interface location.
    next: u32,
}

impl LocationAllocator {
    /// Allocates the next interface location.
    ///
    /// # Errors
    ///
    /// Returns an error when the allocator exceeds [`LocationIndex::MAX`].
    pub fn allocate(&mut self) -> ShaderResult<InterfaceLocation> {
        let location = InterfaceLocation::new(self.next)?;
        self.next += 1;
        Ok(location)
    }
}

/// Typed shader interface location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceLocation {
    /// Validated renderer-neutral location index.
    index: LocationIndex,
}

impl InterfaceLocation {
    /// Creates a validated interface location.
    ///
    /// # Errors
    ///
    /// Returns an error when the location is outside the renderer-neutral
    /// location range.
    pub fn new(index: u32) -> ShaderResult<Self> {
        Ok(Self {
            index: LocationIndex::new(index)?,
        })
    }

    /// Returns the numeric location value.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index.index()
    }
}

/// Descriptor binding assigned to a legalized resource declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorBinding {
    /// Validated descriptor set index.
    set: BindingSet,
    /// Validated descriptor binding index within the set.
    binding: BindingIndex,
}

impl DescriptorBinding {
    /// Creates a validated descriptor binding.
    ///
    /// # Errors
    ///
    /// Returns an error when either set or binding is outside the accepted
    /// renderer-neutral range.
    pub fn new(set: u32, binding: u32) -> ShaderResult<Self> {
        Ok(Self {
            set: BindingSet::new(set)?,
            binding: BindingIndex::new(binding)?,
        })
    }

    /// Returns the descriptor set.
    #[must_use]
    pub const fn set(self) -> u32 {
        self.set.set()
    }

    /// Returns the binding index.
    #[must_use]
    pub const fn binding(self) -> u32 {
        self.binding.binding()
    }
}
