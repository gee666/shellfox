use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct FileLayout {
    pub width: f32,
    pub browser_height: f32,
}
impl Default for FileLayout {
    fn default() -> Self {
        Self {
            width: 420.,
            browser_height: 220.,
        }
    }
}
impl FileLayout {
    pub fn read(cx: &gpui::App) -> Self {
        crate::device_prefs::get(cx)
            .ok()
            .and_then(|store| store.read_files_layout().ok().flatten())
            .and_then(|bytes| decode(&bytes))
            .unwrap_or_default()
    }
    pub fn save(self, cx: &gpui::App) {
        if let (Ok(store), Ok(bytes)) = (crate::device_prefs::get(cx), serde_json::to_vec(&self)) {
            let _ = store.save_files_layout(&bytes);
        }
    }
}

fn decode(bytes: &[u8]) -> Option<FileLayout> {
    if bytes.len() > 128 {
        return None;
    }
    let layout: FileLayout = serde_json::from_slice(bytes).ok()?;
    [layout.width, layout.browser_height]
        .into_iter()
        .all(|value| (0. ..=65536.).contains(&value))
        .then_some(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferred_sizes_are_bounded_and_versioned_independently_from_colors() {
        let layout = decode(br#"{"width":400,"browserHeight":250}"#).unwrap();
        assert_eq!((layout.width, layout.browser_height), (400., 250.));
        assert!(decode(br#"{"width":-1,"browserHeight":220}"#).is_none());
        assert!(decode(br#"{"width":400}"#).is_none());
    }
}
