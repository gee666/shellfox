use gpui::{Image, ImageFormat};
use std::sync::Arc;
#[path = "files_avif.rs"]
mod avif;

pub(crate) enum Preview {
    Binary {
        path: String,
        size: u64,
    },
    Image {
        path: String,
        revision: String,
        mime_type: String,
        image: Option<Arc<Image>>,
    },
}

impl Preview {
    pub fn image(path: String, mime_type: &str, revision: String, bytes: Vec<u8>) -> Self {
        Self::Image {
            path,
            revision,
            mime_type: mime_type.to_owned(),
            image: raster_image(mime_type, bytes),
        }
    }

    pub fn editable(&self) -> bool {
        matches!(self, Self::Image { mime_type, image: Some(_), .. } if matches!(mime_type.as_str(), "image/png" | "image/jpeg" | "image/webp"))
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Binary { path, .. } | Self::Image { path, .. } => path,
        }
    }
}

pub(super) fn raster_image(mime_type: &str, mut bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = if matches!(mime_type, "image/x-icon" | "image/avif") {
        let decoded = if mime_type == "image/avif" {
            avif::decode(&bytes)?
        } else {
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico).ok()?
        };
        bytes.clear();
        decoded
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .ok()?;
        ImageFormat::Png
    } else {
        ImageFormat::from_mime_type(mime_type)?
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}
