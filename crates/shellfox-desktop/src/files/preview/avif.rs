use avif_pixels::{Decoder, Image};
use image::{DynamicImage, ImageBuffer};
use std::io::Cursor;

pub(super) fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    let file = avif_parse::AvifData::from_reader(&mut Cursor::new(bytes)).ok()?;
    let mut decoder = Decoder::new(2, 64_000_000).ok()?;
    let frame = decoder
        .decode(
            &file.primary_item,
            file.alpha_item.as_deref(),
            file.premultiplied_alpha,
        )
        .ok()?;
    macro_rules! image_buffer {
        ($pixels:ident, $variant:ident, $p:ident, $channels:expr) => {{
            let width = u32::try_from($pixels.width()).ok()?;
            let height = u32::try_from($pixels.height()).ok()?;
            let samples = $pixels.pixels().flat_map(|$p| $channels).collect();
            DynamicImage::$variant(ImageBuffer::from_raw(width, height, samples)?)
        }};
    }
    Some(match frame {
        Image::Rgb8(pixels) => image_buffer!(pixels, ImageRgb8, p, [p.r, p.g, p.b]),
        Image::Rgb16(pixels) => image_buffer!(pixels, ImageRgb16, p, [p.r, p.g, p.b]),
        Image::Rgba8(pixels) => image_buffer!(pixels, ImageRgba8, p, [p.r, p.g, p.b, p.a]),
        Image::Rgba16(pixels) => image_buffer!(pixels, ImageRgba16, p, [p.r, p.g, p.b, p.a]),
        Image::Gray8(pixels) => image_buffer!(pixels, ImageLuma8, p, [p.value()]),
        Image::Gray16(pixels) => image_buffer!(pixels, ImageLuma16, p, [p.value()]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_avif_for_preview_without_exposing_an_avif_editor() {
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            10,
            6,
            image::Rgb([40, 110, 200]),
        ));
        let mut raw = Vec::new();
        source
            .write_to(&mut Cursor::new(&mut raw), image::ImageFormat::Avif)
            .unwrap();
        let result = decode(&raw).unwrap();
        assert_eq!((result.width(), result.height()), (10, 6));
        assert!(decode(b"bad input").is_none());
    }
}
