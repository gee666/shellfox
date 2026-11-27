//! Titles a running program publishes with OSC 0/2. As in 0.9, a tab without
//! an explicit name shows the current one; explicit names never pass here.
use shellfox_runtime::{Phase, Runtime, SourceKind};

/// The saved tab title limit, in UTF-16 units as 0.9 counted it.
const MAX_UNITS: usize = 200;

/// The live runtime's published title, made safe to display.
pub(crate) fn published(runtime: &Runtime) -> Option<String> {
    if runtime.source_kind() != SourceKind::OwnedPty || runtime.phase() != Phase::Active {
        return None;
    }
    let metadata = runtime
        .metadata()
        .filter(|metadata| metadata.identity == runtime.identity())?;
    display(metadata.title.as_deref()?, conpty_owner_image())
}

/// 0.9 screen-mirror rule: drop C0/C1 controls, trim, keep the first 200
/// UTF-16 units without splitting a surrogate pair. Blank means no title.
fn display(raw: &str, conpty_owner_image: Option<&str>) -> Option<String> {
    let stripped: String = raw.chars().filter(|c| !c.is_control()).collect();
    let stripped = stripped.trim();
    // System ConPTY can announce the owner's executable as its initial title.
    // It is not a shell title.
    if conpty_owner_image.is_some_and(|image| {
        windows_image_path(stripped).eq_ignore_ascii_case(windows_image_path(image))
    }) {
        return None;
    }
    let mut units = 0;
    let bounded: String = stripped
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_UNITS
        })
        .collect();
    let title = bounded.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

fn conpty_owner_image() -> Option<&'static str> {
    #[cfg(windows)]
    {
        static IMAGE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        IMAGE
            .get_or_init(|| {
                std::env::current_exe()
                    .ok()?
                    .into_os_string()
                    .into_string()
                    .ok()
            })
            .as_deref()
    }
    #[cfg(not(windows))]
    None
}

fn windows_image_path(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::display;

    #[test]
    fn titles_lose_controls_and_are_bounded_like_saved_titles() {
        assert_eq!(display(" Work ", None).as_deref(), Some("Work"));
        assert_eq!(
            display("one\ntwo\u{9b}\u{7f}", None).as_deref(),
            Some("onetwo")
        );
        for blank in ["", "  ", "\u{1b}\u{7}", " \u{85} "] {
            assert_eq!(display(blank, None), None);
        }
        let long = display(&"x".repeat(250), None).unwrap();
        assert_eq!(long.len(), 200);
        // A pair crossing the limit is dropped whole, never split.
        let emoji = display(&format!("{}\u{1f98a}", "x".repeat(199)), None).unwrap();
        assert_eq!(emoji, "x".repeat(199));
        let fits = display(&format!("{}\u{1f98a}", "x".repeat(198)), None).unwrap();
        assert_eq!(fits.encode_utf16().count(), 200);
        for title in [long, emoji, fits, "✳ Claude Code".into()] {
            assert!(shellfox_core::validation::title(&title).is_ok());
        }
    }

    #[test]
    fn conpty_owner_title_is_hidden_without_hiding_application_titles() {
        let image = r"C:\Apps\Shellfox\shellfox.exe";
        let shown = |title| display(title, Some(image));
        assert_eq!(shown(image), None);
        assert_eq!(shown(r"\\?\c:\apps\shellfox\SHELLFOX.EXE"), None);
        assert_eq!(shown("Build work").as_deref(), Some("Build work"));
        assert_eq!(
            shown(r"C:\Projects\work").as_deref(),
            Some(r"C:\Projects\work")
        );
        assert_eq!(display(image, None).as_deref(), Some(image));
    }
}
