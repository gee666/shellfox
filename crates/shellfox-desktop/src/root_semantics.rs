use super::*;
use crate::accessibility::{Role, SemanticActionKind, SemanticError, SemanticNode, SemanticTree};
use gpui::{AnyElement, IntoElement};

impl Root {
    /// Admission only. Services start after the queued native show and viewport verification finish.
    pub(crate) fn queue_presentation(
        &mut self,
        window: &mut Window,
        plan: &crate::accessibility::HiddenWindowPresentation,
        cx: &mut Context<Self>,
    ) -> Result<(), SemanticError> {
        let task = self
            .native_accessibility
            .as_mut()
            .ok_or_else(|| SemanticError::Native("Native provider did not attach.".into()))?
            .queue_present_hidden(window, plan, cx)?;
        cx.spawn_in(window, async move |root, cx| {
            // The retained task runs only after this Root/Window update has returned.
            let result = task.await;
            let _ = root.update_in(cx, |root, window, cx| match result {
                Ok(()) => {
                    window.refresh();
                    root.start_services(window, cx);
                }
                Err(error) => {
                    eprintln!("Native window presentation failed: {error}");
                    crate::app_startup::record_init_failure(error.into(), cx);
                    window.remove_window();
                }
            });
        })
        .detach();
        Ok(())
    }
    pub(crate) fn present_hidden(
        &mut self,
        window: &Window,
        plan: &crate::accessibility::HiddenWindowPresentation,
        cx: &gpui::App,
    ) -> Result<(), SemanticError> {
        self.native_accessibility
            .as_mut()
            .ok_or_else(|| SemanticError::Native("Native provider did not attach.".into()))?
            .present_hidden(window, plan, cx)
    }
    pub(crate) fn sync_semantic_children(&self, cx: &mut Context<Self>) {
        let frame = self.semantic_context.frame();
        let root = self.semantic_context.root_id();
        for view in self.views.values().chain(self.fixture.iter()) {
            let key = format!("terminal/{:?}", view.entity_id());
            if let Err(error) = view.update(cx, |view, cx| {
                view.set_semantic_frame(frame.clone(), root, key, cx)
            }) {
                frame.reject(error);
            }
        }
        if let Some(panel) = &self.settings_panel
            && let Err(error) =
                panel.update(cx, |panel, cx| panel.set_frame(frame.clone(), root, cx))
        {
            frame.reject(error);
        }
        let parent = match &self.dialog {
            None | Some(Dialog::RenameTab { .. } | Dialog::RenameSession { .. }) => root,
            Some(_) => match frame.id_for("root/dialog") {
                Ok(id) => id,
                Err(error) => {
                    frame.reject(error);
                    return;
                }
            },
        };
        match &self.dialog {
            Some(Dialog::Create(draft)) => {
                if let Err(error) = draft.path.update(cx, |field, cx| {
                    field.set_frame(frame.clone(), parent, "Folder path", cx)
                }) {
                    frame.reject(error);
                }
            }
            Some(Dialog::SessionEnvironment { text, .. }) => {
                if let Err(error) = text.update(cx, |field, cx| field.set_frame(frame.clone(), parent, "Environment variables", cx)) {
                    frame.reject(error);
                }
            }
            Some(Dialog::RenameSession { title, .. }) => {
                if let Err(error) = title.update(cx, |field, cx| {
                    field.set_frame(frame.clone(), parent, "Session title", cx)
                }) {
                    frame.reject(error);
                }
            }
            Some(Dialog::RenameTab { title, .. }) => {
                if let Err(error) = title.update(cx, |field, cx| {
                    field.set_frame(frame.clone(), parent, "Terminal tab name", cx)
                }) {
                    frame.reject(error);
                }
            }
            _ => {}
        }
        if let Some(files) = &self.files {
            let parent = match frame.id_for("root/files") {
                Ok(id) => id,
                Err(error) => {
                    frame.reject(error);
                    return;
                }
            };
            if let Err(error) = files.location.update(cx, |field, cx| {
                field.set_frame(frame.clone(), parent, "Folder path", cx)
            }) {
                frame.reject(error);
            }
            if let Some(editor) = &files.editor
                && let Err(error) = editor.text.update(cx, |field, cx| {
                    field.set_frame(frame.clone(), parent, "File contents", cx)
                })
            {
                frame.reject(error);
            }
        }
    }
    pub(crate) fn semantic_region<E: IntoElement>(
        &self,
        key: &str,
        role: Role,
        name: impl Into<String>,
        child: E,
    ) -> AnyElement {
        let frame = self.semantic_context.frame();
        match frame.id_for(key) {
            Ok(id) => {
                let mut node = SemanticNode::new(id, Some(frame.root_id()), role, name);
                if matches!(role, Role::Dialog | Role::AlertDialog) {
                    node.data.set_modal();
                }
                frame.wrap(node, child).into_any_element()
            }
            Err(error) => {
                frame.reject(error);
                child.into_any_element()
            }
        }
    }
    pub(crate) fn finish_semantic_root<E: IntoElement>(
        &self,
        child: E,
        cx: &Context<Self>,
    ) -> AnyElement {
        if self.native_accessibility.is_none() {
            return child.into_any_element();
        }
        let frame = self.semantic_context.frame();
        let node = SemanticNode::new(
            frame.root_id(),
            None,
            Role::Window,
            self.window_caption.clone(),
        );
        let root = cx.weak_entity();
        frame
            .finish(frame.wrap(node, child), move |tree, window, cx| {
                let _ = root.update(cx, |root, cx| root.publish_semantics(tree, window, cx));
            })
            .into_any_element()
    }
    fn publish_semantics(
        &mut self,
        tree: Result<SemanticTree, SemanticError>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let result = tree.and_then(|tree| {
            self.native_accessibility
                .as_mut()
                .ok_or_else(|| SemanticError::Native("Native provider unavailable.".into()))?
                .publish(tree, window)
        });
        if let Err(error) = result {
            // Do not leave an old native tree representing a different dialog/document.
            self.native_accessibility = None;
            self.error = Some(format!("Native accessibility unavailable: {error}"));
            cx.notify();
        }
    }
    pub(super) fn poll_semantics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = &self.native_accessibility else {
            return;
        };
        let refresh = provider.needs_frame();
        let actions = provider.drain_actions();
        let now = std::time::Instant::now();
        for view in self.views.values().chain(self.fixture.iter()) {
            view.update(cx, |view, cx| view.refresh_semantics(now, cx));
        }
        if refresh {
            cx.notify();
        }
        if self.closing_window {
            return;
        }
        for action in actions {
            if self.handle_quit_semantic_action(&action, window, cx)
                || self.handle_profile_menu_semantic_action(&action, window, cx)
                || self.handle_sidebar_settings_action(&action, window, cx)
                || self.handle_tab_semantic_action(&action, window, cx)
                || self.handle_tab_control_semantic_action(&action, window, cx)
            {
                continue;
            }
            if self.handle_power_action(&action, window, cx) {
                continue;
            }
            if self.settings
                && let Some(panel) = &self.settings_panel
                && panel.update(cx, |panel, cx| {
                    panel.handle_semantic_action(&action, window, cx)
                })
            {
                continue;
            }
            let mut fields = Vec::new();
            match &self.dialog {
                Some(Dialog::Create(draft)) => fields.push(draft.path.clone()),
                Some(Dialog::RenameSession { title, .. } | Dialog::RenameTab { title, .. } | Dialog::SessionEnvironment { text: title, .. }) => {
                    fields.push(title.clone())
                }
                _ => {}
            }
            if self.dialog.is_none()
                && let Some(files) = &self.files
                && files.visible
            {
                fields.push(files.location.clone());
                if let Some(editor) = &files.editor {
                    fields.push(editor.text.clone());
                }
            }
            if fields.iter().any(|field| {
                field.update(cx, |field, cx| {
                    field.handle_semantic_action(&action, window, cx)
                })
            }) {
                continue;
            }
            if self.settings || self.dialog.is_some() {
                continue;
            }
            let covered =
                self.files.as_ref().is_some_and(|files| files.visible) && self.files_narrow(window);
            if !covered
                && let Some(view) = self.selected_tab().and_then(|id| self.views.get(&id))
                && view.update(cx, |view, cx| {
                    view.handle_terminal_semantic_action(&action, window, cx)
                })
            {
                continue;
            }
            // Do not focus or mutate an old/covered terminal via the generic control fallback.
            if action.terminal_stamp.is_some()
                || matches!(action.kind, SemanticActionKind::SetTextSelection(_))
            {
                cx.notify();
                continue;
            }
            if matches!(action.kind, SemanticActionKind::Focus)
                && let Some(handle) = self.semantic_context.frame().focus_handle(action.target)
            {
                handle.focus(window);
                cx.notify();
            }
        }
    }
}
