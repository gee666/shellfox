use crate::{
    accessibility::{Action, Role, SemanticAction, SemanticActionKind, SemanticNode},
    editor_composition::EditorOutcome,
    files::{pending::FileAction, transfer::TransferDirection},
    root::Root,
};
use gpui::{AnyElement, Context, FocusHandle, IntoElement, Window};

#[derive(Clone, Copy)]
pub(super) enum Control {
    Browse,
    SubmitPath,
    Browser,
    Close,
    Maximize,
    CopyPath,
    Upload,
    Download,
    CancelTransfer,
}

impl Control {
    const ALL: [Self; 9] = [
        Self::Browse,
        Self::SubmitPath,
        Self::Browser,
        Self::Close,
        Self::Maximize,
        Self::CopyPath,
        Self::Upload,
        Self::Download,
        Self::CancelTransfer,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::Browse => "files/browse",
            Self::SubmitPath => "files/submit-path",
            Self::Browser => "files/browser-toggle",
            Self::Close => "files/close",
            Self::Maximize => "files/maximize",
            Self::CopyPath => "files/copy-path",
            Self::Upload => "files/upload",
            Self::Download => "files/download",
            Self::CancelTransfer => "files/cancel-transfer",
        }
    }

    pub(super) fn wrap<E: IntoElement>(
        self,
        root: &Root,
        name: &'static str,
        enabled: bool,
        focus: Option<&FocusHandle>,
        element: E,
    ) -> AnyElement {
        let frame = root.semantic_context.frame();
        match (frame.id_for(self.key()), frame.id_for("root/files")) {
            (Ok(id), Ok(parent)) => {
                let mut node = SemanticNode::new(id, Some(parent), Role::Button, name);
                if enabled {
                    node.data.add_action(Action::Click);
                    node.focus = focus.cloned();
                } else {
                    node.data.set_disabled();
                }
                frame.wrap(node, element).into_any_element()
            }
            (Err(error), _) | (_, Err(error)) => {
                frame.reject(error);
                element.into_any_element()
            }
        }
    }
}

impl Root {
    pub(crate) fn handle_files_control(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(action.kind, SemanticActionKind::Click)
            || self.settings
            || self.dialog.is_some()
            || !self.files_source_current()
        {
            return false;
        }
        let Some(files) = self
            .files
            .as_ref()
            .filter(|files| files.visible && files.pending_file_action.is_none())
        else {
            return false;
        };
        let frame = self.semantic_context.frame();
        let Some(control) = Control::ALL
            .into_iter()
            .find(|control| frame.id_for(control.key()).ok() == Some(action.target))
        else {
            return false;
        };
        let can_transfer = !files.backend.is_local()
            && files.transfer.is_none()
            && files.listing.is_some()
            && matches!(
                cx.try_global::<EditorOutcome>(),
                Some(EditorOutcome::Ready(_))
            );
        match control {
            Control::Browse if files.browser && files.backend.is_local() && !files.busy => {
                files.browser_focus.browse.focus(window);
                self.browse_files(window, cx);
            }
            Control::SubmitPath if files.browser && !files.busy => self.submit_file_path(cx),
            Control::Browser => self.toggle_file_browser(window, cx),
            Control::Close => self.request_file_action(FileAction::Hide, cx),
            Control::Maximize if !self.files_narrow(window) || files.maximized => {
                self.toggle_file_maximized(window, cx)
            }
            Control::CopyPath if files.opened_path().is_some() => self.copy_open_file_path(cx),
            Control::Upload if can_transfer => {
                self.begin_file_transfer(TransferDirection::Upload, window, cx)
            }
            Control::Download
                if can_transfer
                    && files
                        .opened_path()
                        .or(files.document_path.as_deref())
                        .is_some() =>
            {
                self.begin_file_transfer(TransferDirection::Download, window, cx);
            }
            Control::CancelTransfer if files.transfer.is_some() => self.cancel_file_transfer(cx),
            _ => return false,
        }
        true
    }
}
