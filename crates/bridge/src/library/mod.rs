//! Workshop library metadata types, scanner, and thumbnail cache.

pub mod entry;
pub mod filter;
pub mod scanner;
pub mod thumbnails;

pub use entry::WallpaperEntry;
pub use filter::TypeFilter;
pub use scanner::{resolve_preview, scan};
pub use thumbnails::{
    CACHE_CAPACITY, THUMB_SIDE, ThumbKey, ThumbnailCache, ThumbnailRgba, decode_and_crop,
};
