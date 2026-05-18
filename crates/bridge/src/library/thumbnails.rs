use std::{
    collections::{HashMap, VecDeque},
    fs,
    hash::Hash,
    io::Cursor,
    path::Path,
    time::SystemTime,
};

use image::{AnimationDecoder, ImageError, ImageFormat, imageops};

pub const THUMB_SIDE: u32 = 200;
pub const CACHE_CAPACITY: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThumbKey {
    pub id: String,
    pub mtime: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbnailRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct ThumbnailCache {
    cap: usize,
    entries: HashMap<ThumbKey, ThumbnailRgba>,
    order: VecDeque<ThumbKey>,
}

impl ThumbnailCache {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn get(&mut self, key: &ThumbKey) -> Option<ThumbnailRgba> {
        let thumbnail = self.entries.get(key)?.clone();
        self.touch(key);
        Some(thumbnail)
    }

    /// Non-mutating lookup; does not refresh recency.
    #[must_use]
    pub fn peek(&self, key: &ThumbKey) -> Option<ThumbnailRgba> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, key: ThumbKey, thumbnail: ThumbnailRgba) {
        if self.cap == 0 {
            return;
        }

        if self.entries.insert(key.clone(), thumbnail).is_some() {
            self.remove_ordered(&key);
        }

        self.order.push_back(key);
        self.evict_if_needed();
    }

    fn touch(&mut self, key: &ThumbKey) {
        self.remove_ordered(key);
        self.order.push_back(key.clone());
    }

    fn remove_ordered(&mut self, key: &ThumbKey) {
        if let Some(pos) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(pos);
        }
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

/// # Errors
///
/// Returns an error when the image file cannot be read or decoded.
pub fn decode_and_crop(path: &Path) -> Result<ThumbnailRgba, ImageError> {
    let bytes = fs::read(path)?;
    let format = image::guess_format(&bytes)?;
    let rgba = match format {
        ImageFormat::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
            let mut frames = decoder.into_frames();
            let frame = frames
                .next()
                .transpose()?
                .ok_or_else(|| ImageError::IoError(std::io::Error::other("empty GIF")))?;
            frame.into_buffer()
        }
        _ => image::load_from_memory_with_format(&bytes, format)?.into_rgba8(),
    };
    let side = rgba.width().min(rgba.height());
    let left = (rgba.width() - side) / 2;
    let top = (rgba.height() - side) / 2;
    let cropped = imageops::crop_imm(&rgba, left, top, side, side).to_image();
    let resized = imageops::resize(
        &cropped,
        THUMB_SIDE,
        THUMB_SIDE,
        imageops::FilterType::Lanczos3,
    );

    Ok(ThumbnailRgba {
        width: THUMB_SIDE,
        height: THUMB_SIDE,
        rgba: resized.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        time::SystemTime,
    };

    use image::{Frame, ImageBuffer, Rgba};
    use tempfile::TempDir;

    use super::{THUMB_SIDE, ThumbKey, ThumbnailCache, ThumbnailRgba, decode_and_crop};

    fn key(id: &str) -> ThumbKey {
        ThumbKey {
            id: id.to_string(),
            mtime: Some(SystemTime::UNIX_EPOCH),
        }
    }

    fn thumbnail(bytes: Vec<u8>) -> ThumbnailRgba {
        ThumbnailRgba {
            width: 1,
            height: 1,
            rgba: bytes,
        }
    }

    #[test]
    fn lru_evicts_oldest_entry() {
        let mut cache = ThumbnailCache::new(2);
        cache.insert(key("one"), thumbnail(vec![1, 2, 3, 4]));
        cache.insert(key("two"), thumbnail(vec![5, 6, 7, 8]));
        cache.insert(key("three"), thumbnail(vec![9, 10, 11, 12]));

        assert!(cache.peek(&key("one")).is_none());
        assert!(cache.peek(&key("two")).is_some());
        assert!(cache.peek(&key("three")).is_some());
    }

    #[test]
    fn lru_touches_on_access() {
        let mut cache = ThumbnailCache::new(2);
        cache.insert(key("one"), thumbnail(vec![1, 2, 3, 4]));
        cache.insert(key("two"), thumbnail(vec![5, 6, 7, 8]));

        assert!(cache.get(&key("one")).is_some());

        cache.insert(key("three"), thumbnail(vec![9, 10, 11, 12]));

        assert!(cache.peek(&key("one")).is_some());
        assert!(cache.peek(&key("two")).is_none());
        assert!(cache.peek(&key("three")).is_some());
    }

    #[test]
    fn decode_and_crop_loads_png() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("thumb.png");
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(8, 4, |x, _y| {
            if (2..6).contains(&x) {
                Rgba([255u8, 0, 0, 255])
            } else {
                Rgba([0u8, 0, 255, 255])
            }
        });

        image.save(&path).unwrap();

        let thumbnail = decode_and_crop(&path).unwrap();
        assert_eq!(thumbnail.width, THUMB_SIDE);
        assert_eq!(thumbnail.height, THUMB_SIDE);
        assert!(
            thumbnail
                .rgba
                .chunks_exact(4)
                .take(8)
                .all(|pixel| pixel == [255, 0, 0, 255])
        );

        assert!(fs::metadata(&path).is_ok());
    }

    #[test]
    fn decode_and_crop_uses_first_gif_frame() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("thumb.gif");
        {
            let mut file = File::create(&path).unwrap();
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut file);

            let first = ImageBuffer::from_pixel(8, 4, Rgba([255u8, 0, 0, 255]));
            let second = ImageBuffer::from_pixel(8, 4, Rgba([0u8, 0, 255, 255]));
            encoder.encode_frame(Frame::new(first)).unwrap();
            encoder.encode_frame(Frame::new(second)).unwrap();
        }

        let thumbnail = decode_and_crop(&path).unwrap();
        assert_eq!(thumbnail.width, THUMB_SIDE);
        assert_eq!(thumbnail.height, THUMB_SIDE);
        assert_eq!(thumbnail.rgba[0], 255);
        assert_eq!(thumbnail.rgba[1], 0);
        assert_eq!(thumbnail.rgba[2], 0);
        assert_eq!(thumbnail.rgba[3], 255);
    }

    #[test]
    fn insert_replacing_existing_key_updates_recency() {
        let mut cache = ThumbnailCache::new(2);
        let one_initial = thumbnail(vec![1, 2, 3, 4]);
        let one_updated = thumbnail(vec![5, 6, 7, 8]);
        let two = thumbnail(vec![9, 10, 11, 12]);
        let three = thumbnail(vec![13, 14, 15, 16]);

        cache.insert(key("one"), one_initial);
        cache.insert(key("two"), two);
        cache.insert(key("one"), one_updated.clone());
        cache.insert(key("three"), three);

        assert_eq!(cache.peek(&key("one")), Some(one_updated));
        assert!(cache.peek(&key("two")).is_none());
        assert!(cache.peek(&key("three")).is_some());
    }
}
