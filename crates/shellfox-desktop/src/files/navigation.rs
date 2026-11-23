use crate::{
    files::{list::Navigation, terminal_links::LinkOrigin},
    root::Root,
};
use gpui::{Context, Window};
use shellfox_files::EntryKind;

impl Root {
    pub(crate) fn open_terminal_folder(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_tab() else {
            return;
        };
        if !self.tabs.views.contains_key(&id) {
            return;
        }
        let Some(path) = self
            .tabs.views
            .get(&id)
            .and_then(|view| view.read(cx).working_directory())
        else {
            self.error = Some("The shell has not reported a local current directory. Open Files to browse manually.".into());
            cx.notify();
            return;
        };
        let Some((backend, _)) = self.selected_document_backend(cx) else {
            return;
        };
        let visible = self.files.panel.as_ref().is_some_and(|files| files.visible);
        self.select_file_scope(backend, cx);
        if let Some(files) = &mut self.files.panel {
            files.visible = visible;
        }
        self.navigate_files(
            path,
            Navigation::Terminal(LinkOrigin::Local(self.workspace.navigation.epoch())),
            cx,
        );
    }

    pub(crate) fn open_peer_terminal_folder(&mut self, cx: &mut Context<Self>) {
        let Some((peer, state)) = self.peers.selected_peer() else {
            return;
        };
        let (Some(tab), Some(&navigation)) =
            (state.selected_tab, self.peer_ui.navigation.get(peer))
        else {
            return;
        };
        let origin = LinkOrigin::Peer {
            tab,
            navigation,
            source: self.source_selection_epoch(),
        };
        if !self.link_is_current(origin) {
            return;
        }
        let Ok((backend, cwd)) = self.peer_file_backend() else {
            return;
        };
        let path = if backend.is_wsl() {
            ".".into()
        } else if let Some(cwd) = cwd {
            cwd
        } else {
            self.error = Some("The remote tab has no reported current directory.".into());
            cx.notify();
            return;
        };
        self.select_file_scope(backend, cx);
        self.navigate_files(path, Navigation::Terminal(origin), cx);
    }

    pub(crate) fn toggle_file_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        if let Some(files) = &mut self.files.panel {
            if files.browser {
                files.completion.state.dismiss();
                if files.browser_focus.region.contains_focused(window, cx)
                    || files.height_focus.is_focused(window)
                {
                    window.focus(&files.browser_focus.toggle);
                }
            }
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
        if let Some(files) = &mut self.files.panel {
            if files.maximized {
                files.maximized = false;
                files.browser = files.browser_before_maximize;
            } else {
                files.browser_before_maximize = files.browser;
                files.completion.state.dismiss();
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
        let Some(files) = &self.files.panel else { return };
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
        let prompt = crate::files::view::picker::open(window, cx, gpui::PathPromptOptions {
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
                    Ok(Some(path)) => {
                        if backend.is_wsl() && !root.use_host_file_scope(cx) {
                            return;
                        }
                        root.list_files(path.to_string_lossy().into_owned(), cx);
                    }
                    Ok(None) => {}
                    Err(_) => root.error = Some("Folder picker failed.".into()),
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
        let Some(files) = &self.files.panel else {
            return;
        };
        let path = if file {
            files
                .opened_path()
                .or(files.document_path.as_deref())
                .map(str::to_owned)
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
        let path = self.files.panel.as_ref().and_then(|files| {
            files
                .opened_path()
                .or(files.document_path.as_deref())
                .map(str::to_owned)
        });
        if let Some(path) = path {
            self.request_file_action(crate::files::pending::FileAction::Open(path), cx);
        }
    }
    pub(crate) fn file_editor_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape"
            && let Some(draft) = self.image_draft()
            && draft.crop.take().is_some()
        {
            draft.anchor = None;
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let mods = event.keystroke.modifiers;
        let command = if cfg!(target_os = "macos") {
            mods.platform && !mods.control && !mods.alt
        } else {
            mods.control && !mods.platform && !mods.alt
        };
        if event.keystroke.key == "s"
            && command
            && !self
                .files.panel
                .as_ref()
                .and_then(|files| files.editor.as_ref())
                .is_some_and(|editor| editor.text.read(cx).is_composing())
        {
            if self
                .files.panel
                .as_ref()
                .is_some_and(|files| files.image_editor.is_some())
            {
                self.save_edited_image(cx);
            } else {
                self.save_file(cx);
            }
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
        let key = event.keystroke.key.as_str();
        if self
            .files.panel
            .as_ref()
            .is_some_and(|files| files.location.read(cx).is_composing())
        {
            if key == "tab" {
                window.prevent_default();
                cx.stop_propagation();
            }
            return;
        }
        if self.file_completion_key(key, event.keystroke.modifiers.shift, window, cx) {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        let Some(files) = &mut self.files.panel else { return };
        match key {
            "escape" if files.missing_folder.is_some() => {
                files.missing_folder = None;
                files.location.read(cx).focus(window);
                cx.notify();
            }
            "enter" if files.missing_folder.is_none() => self.submit_file_path(cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(crate) fn submit_file_path(&mut self, cx: &mut Context<Self>) {
        let Some(files) = &self.files.panel else { return };
        let location = files.location.read(cx);
        if files.busy || files.missing_folder.is_some() || location.is_composing() {
            return;
        }
        let path = location.text.trim();
        let path = if path.is_empty() { "~" } else { path }.to_owned();
        if files.backend.is_wsl() && (path == "~" || path.starts_with("~/")) {
            if self.use_host_file_scope(cx) {
                self.navigate_files(path, Navigation::Input, cx);
            }
        } else {
            self.navigate_files(path, Navigation::Input, cx);
        }
    }
}
