use crate::root::Root;
use gpui::{Context, Window};
use shellfox_files::EntryKind;

impl Root {
    pub(crate) fn toggle_file_browser(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        if let Some(files) = &mut self.files {
            files.browser = !files.browser;
            if files.maximized {
                files.browser_before_maximize = files.browser;
            }
        }
        cx.notify();
    }
    pub(crate) fn toggle_file_maximized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        if let Some(files) = &mut self.files {
            if files.maximized {
                files.maximized = false;
                files.browser = files.browser_before_maximize;
            } else {
                files.browser_before_maximize = files.browser;
                files.browser = false;
                files.maximized = true;
            }
            if let Some(editor) = &files.editor {
                editor.text.read(cx).focus(window);
            } else {
                files.location.read(cx).focus(window);
            }
        }
        cx.notify();
    }
    pub(crate) fn browse_files(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &self.files else { return };
        if !files.backend.is_local() {
            self.error = Some(
                "Enter a folder path on the selected peer; the local OS picker cannot browse it."
                    .into(),
            );
            cx.notify();
            return;
        }
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder".into()),
        });
        cx.spawn_in(window, async move |root, cx| {
            let result = prompt.await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root.files_result_current(epoch, &backend) {
                    root.error = Some("Folder selection belongs to a retired local source.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            root.list_files(path.to_string_lossy().into_owned(), cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => root.error = Some("Folder picker failed.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn open_file_explorer(&mut self, file: bool, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &self.files else {
            return;
        };
        let path = if file {
            files.editor.as_ref().map(|editor| editor.file.path.clone())
        } else {
            files.listing.as_ref().map(|listing| listing.path.clone())
        };
        let Some(path) = path else {
            return;
        };
        if !files.backend.is_local() {
            self.error =
                Some("A peer path cannot be opened in this machine's file explorer.".into());
            cx.notify();
            return;
        }
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.local_stat_for_explorer(&path) });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                if !root.files_result_current(epoch, &backend) {
                    root.error = Some("Explorer result belongs to a retired source.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(stat) => {
                        if stat.kind == EntryKind::Directory {
                            match url::Url::from_directory_path(&stat.path) {
                                Ok(uri) => cx.open_url(uri.as_str()),
                                Err(()) => {
                                    root.error = Some(
                                        "Cannot open this directory in the system explorer.".into(),
                                    )
                                }
                            }
                        } else if stat.kind == EntryKind::File {
                            cx.reveal_path(std::path::Path::new(&stat.path));
                        } else {
                            root.error = Some("Unsupported file type.".into());
                        }
                    }
                    Err(error) => root.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn reload_file(&mut self, cx: &mut Context<Self>) {
        let path = self
            .files
            .as_ref()
            .and_then(|files| files.editor.as_ref().map(|editor| editor.file.path.clone()));
        if let Some(path) = path {
            self.request_open_file(path, cx);
        }
    }
    pub(crate) fn file_editor_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mods = event.keystroke.modifiers;
        let command = if cfg!(target_os = "macos") {
            mods.platform && !mods.control && !mods.alt
        } else {
            mods.control && !mods.platform && !mods.alt
        };
        if event.keystroke.key == "s"
            && command
            && !self
                .files
                .as_ref()
                .and_then(|files| files.editor.as_ref())
                .is_some_and(|editor| editor.text.read(cx).is_composing())
        {
            self.save_file(cx);
            window.prevent_default();
            cx.stop_propagation();
        }
    }
    pub(crate) fn file_path_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "enter" {
            return;
        }
        let Some(files) = &self.files else {
            return;
        };
        if files.location.read(cx).is_composing() {
            return;
        }
        let path = files.location.read(cx).text.clone();
        self.list_files(path, cx);
        window.prevent_default();
        cx.stop_propagation();
    }
}
