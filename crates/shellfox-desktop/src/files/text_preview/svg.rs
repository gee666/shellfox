use anyhow::{Context, Result, ensure};
use gpui::{Image, ImageFormat};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{ImageHrefResolver, Options, Tree},
};
use std::sync::{Arc, OnceLock};

pub(super) fn rasterize(text: &str) -> Result<Arc<Image>> {
    // An SVG opened as an image has no network or local-file access in Electron.
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let options = Options {
        fontdb: FONTS
            .get_or_init(|| {
                let mut fonts = resvg::usvg::fontdb::Database::new();
                fonts.load_system_fonts();
                Arc::new(fonts)
            })
            .clone(),
        image_href_resolver: ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            resolve_data: ImageHrefResolver::default_data_resolver(),
        },
        ..Options::default()
    };
    let tree = Tree::from_str(text, &options).context("Could not display image.")?;
    let size = tree.size();
    let scale = (4096. / size.width().max(size.height())).min(1.);
    let width = (scale * size.width()).ceil() as u32;
    let height = (scale * size.height()).ceil() as u32;
    ensure!(width > 0 && height > 0, "Could not display image.");
    let mut bitmap = Pixmap::new(width, height).context("Could not display image.")?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut bitmap.as_mut(),
    );
    let bytes = bitmap.encode_png().context("Could not display image.")?;
    Ok(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_raster_and_invalid_input() {
        let img = rasterize(r##"<svg xmlns="http://www.w3.org/2000/svg" width="9000" height="4500"><rect width="9000" height="4500" fill="#fa8072"/></svg>"##).unwrap();
        let decoded = image::load_from_memory(&img.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4096, 2048));
        assert!(rasterize("<html/>").is_err());
    }
}
