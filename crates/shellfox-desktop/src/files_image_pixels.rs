use anyhow::{Result, ensure};
use gpui::{Image, ImageFormat, RenderImage};
use image::{
    DynamicImage, ImageDecoder, ImageEncoder, Limits, RgbaImage,
    codecs::{jpeg, png, webp},
};
use std::{io::Cursor, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy)]
pub(crate) enum Transform {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Crop(Crop),
}

pub(super) fn decode(image: &Image) -> Result<RgbaImage> {
    fn bounded(mut decoder: impl ImageDecoder) -> Result<RgbaImage> {
        let (w, h) = decoder.dimensions();
        ensure!(
            w > 0 && h > 0 && w <= 8192 && h <= 8192 && u64::from(w) * u64::from(h) <= 16_000_000,
            "Image exceeds the 8192-pixel edge or 16-megapixel limit."
        );
        let mut limits = Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        decoder.set_limits(limits)?;
        let orientation = decoder.orientation()?;
        let mut decoded = DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        Ok(decoded.into_rgba8())
    }
    let source = Cursor::new(&image.bytes);
    match image.format {
        ImageFormat::Png => {
            let mut limits = Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            let decoder = png::PngDecoder::with_limits(source, limits)?;
            ensure!(
                !decoder.is_apng()?,
                "Animated PNG editing is not supported."
            );
            bounded(decoder)
        }
        ImageFormat::Webp => {
            let decoder = webp::WebPDecoder::new(source)?;
            ensure!(
                !decoder.has_animation(),
                "Animated WebP editing is not supported."
            );
            bounded(decoder)
        }
        ImageFormat::Jpeg => bounded(jpeg::JpegDecoder::new(source)?),
        _ => anyhow::bail!("Only PNG, JPEG and WebP image saves are supported."),
    }
}

pub(super) fn apply(image: &RgbaImage, transform: Transform) -> Result<RgbaImage> {
    use image::imageops;
    Ok(match transform {
        Transform::RotateLeft => imageops::rotate270(image),
        Transform::RotateRight => imageops::rotate90(image),
        Transform::FlipHorizontal => imageops::flip_horizontal(image),
        Transform::FlipVertical => imageops::flip_vertical(image),
        Transform::Crop(Crop {
            x,
            y,
            width,
            height,
        }) => {
            ensure!(
                width > 0
                    && height > 0
                    && x.checked_add(width).is_some_and(|end| end <= image.width())
                    && y.checked_add(height)
                        .is_some_and(|end| end <= image.height()),
                "Crop must stay inside the image."
            );
            imageops::crop_imm(image, x, y, width, height).to_image()
        }
    })
}

pub(super) fn frame(image: &RgbaImage) -> Arc<RenderImage> {
    let mut bgra = image.clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new([image::Frame::new(bgra)]))
}

pub(super) fn encode(image: &RgbaImage, format: ImageFormat) -> Result<Vec<u8>> {
    use image::ExtendedColorType::{Rgb8, Rgba8};
    let mut bytes = Vec::new();
    let (width, height) = image.dimensions();
    match format {
        ImageFormat::Png => {
            png::PngEncoder::new(&mut bytes).write_image(image, width, height, Rgba8)?
        }
        ImageFormat::Jpeg => {
            let rgb: Vec<_> = image
                .pixels()
                .flat_map(|pixel| {
                    let [r, g, b, a] = pixel.0;
                    let a = u16::from(a);
                    [r, g, b]
                        .map(|value| ((u16::from(value) * a + 255 * (255 - a) + 127) / 255) as u8)
                })
                .collect();
            jpeg::JpegEncoder::new_with_quality(&mut bytes, 92)
                .write_image(&rgb, width, height, Rgb8)?;
        }
        ImageFormat::Webp => {
            webp::WebPEncoder::new_lossless(&mut bytes).write_image(image, width, height, Rgba8)?
        }
        _ => anyhow::bail!("This device cannot encode the original image format."),
    }
    ensure!(
        bytes.len() as u64 <= shellfox_files::TRANSFER_IMAGE_BYTES,
        "Edited image exceeds the 65536 KiB save limit. Crop the image before saving."
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transforms_and_reencodes_without_changing_original() {
        let mut original = RgbaImage::new(3, 2);
        original.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let rotated = apply(&original, Transform::RotateRight).unwrap();
        assert_eq!(rotated.dimensions(), (2, 3));
        assert_eq!(rotated.get_pixel(1, 0), original.get_pixel(0, 0));
        let cropped = apply(
            &rotated,
            Transform::Crop(Crop {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        )
        .unwrap();
        for format in [ImageFormat::Png, ImageFormat::Webp, ImageFormat::Jpeg] {
            let encoded = encode(&cropped, format).unwrap();
            let decoded = decode(&Image::from_bytes(format, encoded)).unwrap();
            assert_eq!(decoded.dimensions(), (1, 2));
        }
        assert!(
            apply(
                &rotated,
                Transform::Crop(Crop {
                    x: 2,
                    y: 0,
                    width: 1,
                    height: 2
                })
            )
            .is_err()
        );
    }
}
