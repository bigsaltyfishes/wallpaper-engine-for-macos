//! Token index ranges.

/// Half-open token-index range in an existing token stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenIndexRange {
    /// First token in the range.
    pub start: usize,
    /// First token outside the range.
    pub end: usize,
}

impl TokenIndexRange {
    /// Creates a half-open token range.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates a range from inclusive token bounds.
    #[must_use]
    pub const fn from_inclusive(start: usize, end: usize) -> Self {
        Self {
            start,
            end: end + 1,
        }
    }

    /// Returns the first token index.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Returns the first token index outside the range.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// Returns the range length.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Returns whether the range is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the final token index when the range is non-empty.
    #[must_use]
    pub const fn last(self) -> Option<usize> {
        if self.is_empty() {
            None
        } else {
            Some(self.end - 1)
        }
    }
}
