//! Cover-art resize / recompress before embedding (#41).
//!
//! Fetched artwork is routinely far larger than a tag needs, and every embedded
//! copy inflates every file in the album. [`resize_cover`] shrinks an oversized
//! image to a maximum dimension and re-encodes it as JPEG at a chosen quality,
//! feeding the same embed path so the result stays previewable and undoable.

use crate::model::CoverArt;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

/// Downscale `art` so its longer side is at most `max_px`, re-encoding as JPEG
/// at `quality` (1..=100). Aspect ratio is preserved.
///
/// Left untouched (a clone) when: `max_px` is 0 (the feature is off), the image
/// already fits within `max_px`, or the bytes can't be decoded — a cover we
/// can't read should still embed as-is rather than fail the whole operation.
pub fn resize_cover(art: &CoverArt, max_px: u32, quality: u8) -> CoverArt {
    if max_px == 0 {
        return art.clone();
    }
    let Ok(image) = image::load_from_memory(&art.data) else {
        return art.clone();
    };
    if image.width() <= max_px && image.height() <= max_px {
        return art.clone();
    }

    // `resize` preserves aspect ratio within the max×max box; Lanczos3 is the
    // quality choice for downscaling photographic art.
    let resized = image.resize(max_px, max_px, FilterType::Lanczos3);

    // Covers are opaque; encode RGB JPEG (drops any alpha channel) — far smaller
    // than PNG for artwork and universally embeddable.
    let rgb = resized.to_rgb8();
    let mut data = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut data, quality.clamp(1, 100));
    if rgb.write_with_encoder(encoder).is_err() {
        return art.clone();
    }
    CoverArt {
        mime: "image/jpeg".to_string(),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    /// A solid-colour PNG of the given size, as bytes.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(width, height, |_, _| Rgb([120u8, 40, 200]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn dimensions(data: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(data).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn downscales_oversized_art_and_reencodes_jpeg() {
        let art = CoverArt {
            mime: "image/png".into(),
            data: png(2000, 1000),
        };
        let out = resize_cover(&art, 500, 85);
        assert_eq!(out.mime, "image/jpeg");
        // Longer side clamped to 500, aspect ratio (2:1) preserved.
        assert_eq!(dimensions(&out.data), (500, 250));
        // A JPEG of a solid colour is far smaller than the source PNG.
        assert!(out.data.len() < art.data.len());
    }

    #[test]
    fn leaves_small_or_disabled_or_undecodable_art_untouched() {
        let small = CoverArt {
            mime: "image/png".into(),
            data: png(300, 300),
        };
        // Already within bounds -> unchanged.
        assert_eq!(resize_cover(&small, 500, 85), small);
        // Disabled (max_px == 0) -> unchanged even when oversized.
        let big = CoverArt {
            mime: "image/png".into(),
            data: png(2000, 2000),
        };
        assert_eq!(resize_cover(&big, 0, 85), big);
        // Undecodable bytes -> unchanged, never an error.
        let junk = CoverArt {
            mime: "image/jpeg".into(),
            data: vec![1, 2, 3, 4],
        };
        assert_eq!(resize_cover(&junk, 500, 85), junk);
    }
}
