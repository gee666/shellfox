//! The browser's "New file" form: an empty file in the browsed folder, never an overwrite.
use super::list::Navigation;
use crate::{
    field::{Field, FieldOptions},
    root::Root,
};
use gpui::{Context, Entity, Window};

pub(crate) struct NewFile {
    pub name: Entity<Field>,
    pub error: Option<String>,
}

/// A single file name, never a path, so creation stays in the browsed folder.
pub(crate) fn file_name_error(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        Some("Enter a file name.")
    } else if matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\') || c.is_ascii_control())
    {
        Some("Enter a file name without folders.")
    } else if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.ends_with(['.', ' ']) {
        Some("This name is not valid on every system.")
    } else {
        None
    }
}

impl Root {
    pub(crate) fn toggle_new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(files) = self.files.panel.as_mut().filter(|files| !files.busy()) else {
            return;
        };
        if files.new_file.take().is_some() {
            cx.notify();
            return;
        }
        if files.listing.is_none() {
            return;
        }
        let name = Field::settings_input(None, cx);
        name.update(cx, |field, cx| {
            field.configure(
                FieldOptions {
                    placeholder: Some("name.txt".into()),
                    ..Default::default()
                },
                cx,
            )
        });
        name.read(cx).focus(window);
        // The missing-folder prompt holds focus while shown; this form replaces it.
        files.missing_folder = None;
        files.new_file = Some(NewFile { name, error: None });
        cx.notify();
    }

    pub(crate) fn new_file_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.files.panel.as_ref().and_then(|f| f.new_file.as_ref()) else {
            return;
        };
        if form.name.read(cx).is_composing() {
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.create_new_file(cx),
            "escape" => self.cancel_new_file(window, cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(crate) fn cancel_new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(files) = self.files.panel.as_mut().filter(|files| !files.busy())
            && files.new_file.take().is_some()
        {
            files.location.read(cx).focus(window);
            cx.notify();
        }
    }

    pub(crate) fn create_new_file(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        let Some(folder) = files.listing.as_ref().map(|list| list.path.clone()) else {
            return;
        };
        if files.busy() {
            return;
        }
        let Some(form) = &mut files.new_file else {
            return;
        };
        let name = form.name.read(cx).text.trim().to_owned();
        let path = match file_name_error(&name) {
            Some(error) => Err(anyhow::anyhow!(error)),
            None => super::transfer::io::destination(&folder, &name),
        };
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                form.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        form.error = None;
        form.name
            .update(cx, |field, cx| field.set_read_only(true, cx));
        let request = files.activity.begin();
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.create_file(&path).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.files_result_current(epoch, &backend);
                let Some(files) = &mut root.files.panel else {
                    return;
                };
                if !files.backend.same_origin(&backend) || !files.activity.finish(request) {
                    return;
                }
                match result {
                    Ok(created) => {
                        files.new_file = None;
                        // Lists the folder again and opens the new file, as a typed path would.
                        if current {
                            root.navigate_files(created, Navigation::Input, cx);
                        }
                    }
                    Err(error) => {
                        if let Some(form) = &mut files.new_file {
                            form.error = Some(error.to_string());
                            form.name
                                .update(cx, |field, cx| field.set_read_only(false, cx));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::file_name_error;

    #[test]
    fn names_follow_the_portable_single_name_rules() {
        for name in ["notes.txt", "Makefile", ".env", "a b.md", "файл.rs"] {
            assert_eq!(file_name_error(name), None, "{name}");
        }
        assert_eq!(file_name_error("  "), Some("Enter a file name."));
        for name in [".", "..", "a/b", "a\\b", "tab\there", "del\u{7f}"] {
            assert_eq!(
                file_name_error(name),
                Some("Enter a file name without folders."),
                "{name}"
            );
        }
        for name in [
            "a:b", "what?", "star*", "pipe|", "q\"", "<x>", "dot.", "space ",
        ] {
            assert_eq!(
                file_name_error(name),
                Some("This name is not valid on every system."),
                "{name}"
            );
        }
    }
}
