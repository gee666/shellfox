use image::{DynamicImage, ImageFormat};

pub(super) fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    image::load_from_memory_with_format(bytes, ImageFormat::Ico)
        .ok()
        .or_else(|| decode_embedded_png(bytes))
}

fn decode_embedded_png(bytes: &[u8]) -> Option<DynamicImage> {
    // image's ICO decoder rejects PNG entries that aren't RGBA8. Browsers
    // also display RGB PNG icons. Accept a bounded, matching PNG entry only.
    if bytes.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = usize::from(u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?));
    let table_end = 6 + count * 16;
    let entries = bytes.get(6..table_end)?;
    let (_, png) = entries
        .chunks_exact(16)
        .filter_map(|entry| {
            let size =
                usize::try_from(u32::from_le_bytes(entry.get(8..12)?.try_into().ok()?)).ok()?;
            let start =
                usize::try_from(u32::from_le_bytes(entry.get(12..16)?.try_into().ok()?)).ok()?;
            if start < table_end {
                return None;
            }
            let png = bytes.get(start..start.checked_add(size)?)?;
            if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
                return None;
            }
            let side = |byte: u8| if byte == 0 { 256 } else { u32::from(byte) };
            let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
            let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
            (width == side(entry[0]) && height == side(entry[1])).then(|| (width * height, png))
        })
        .max_by_key(|(area, _)| *area)?;
    image::load_from_memory_with_format(png, ImageFormat::Png).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn displays_a_non_rgba_png_icon_without_untrusted_dimension_overrides() {
        let color = image::Rgb([14, 180, 97]);
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(10, 6, color));
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, ImageFormat::Ico).unwrap();
        let mut bytes = bytes.into_inner();
        let image = decode(&bytes).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), (10, 6));
        assert_eq!(image.get_pixel(0, 0), &color);
        bytes[6] = 20;
        assert!(decode(&bytes).is_none());
        assert!(decode(b"\x00\x00\x01\x00\xff\xff").is_none());
    }
}
