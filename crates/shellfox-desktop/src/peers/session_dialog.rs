use super::*;
use crate::{
    field::{Field, FieldOptions},
    sidebar::session_menu::Item,
    settings::environment_data,
};
use gpui::FocusHandle;
use shellfox_protocol::TabLifecycle;
use sidebar_actions::Mutation;

pub(crate) enum Editor {
    Rename(Entity<Field>),
    Environment(Entity<Field>),
    Delete,
}

pub(crate) struct PeerSessionDialog {
    pub cycle: source::PeerCycle,
    pub id: SessionId,
    pub editor: Editor,
    prior: Option<FocusHandle>,
}

impl PeerSessionDialog {
    pub(crate) fn field(&self) -> Option<&Entity<Field>> {
        match &self.editor {
            Editor::Rename(field) | Editor::Environment(field) => Some(field),
            Editor::Delete => None,
        }
    }
}

impl Root {
    pub(super) fn open_peer_session_dialog(
        &mut self,
        cycle: source::PeerCycle,
        id: SessionId,
        item: Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() {
            return;
        }
        let Some(session) = self.peer_session(&cycle, id) else {
            return;
        };
        let prior = window.focused(cx);
        let editor = match item {
            Item::Rename => Editor::Rename(Field::session_title(session.title.clone(), cx)),
            Item::Environment => {
                let field = Field::textarea(environment_data::format(&session.env), cx);
                field.update(cx, |field, cx| {
                    field.configure(
                        FieldOptions {
                            placeholder: Some(
                                "# One per line\nAPI_KEY=sk-...\nNODE_ENV=development".into(),
                            ),
                            ..Default::default()
                        },
                        cx,
                    )
                });
                cx.observe(&field, |_, _, cx| cx.notify()).detach();
                Editor::Environment(field)
            }
            Item::Delete => Editor::Delete,
            _ => return,
        };
        let dialog = PeerSessionDialog {
            cycle,
            id,
            editor,
            prior,
        };
        if let Some(field) = dialog.field() {
            field.read(cx).focus(window);
        } else {
            self.modal_focus.focus(window);
        }
        if matches!(&dialog.editor, Editor::Environment(_)) {
            self.sidebar_ui.environment_resize = Default::default();
        }
        self.dialog = Some(Dialog::PeerSession(dialog));
        cx.notify();
    }

    pub(crate) fn confirm_peer_session_dialog(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(Dialog::PeerSession(dialog)) = &self.dialog else {
            return;
        };
        let action = match &dialog.editor {
            Editor::Rename(field) => {
                let title = field.read(cx).text.trim();
                if title.is_empty() {
                    return;
                }
                Mutation::Rename(title.to_owned())
            }
            Editor::Environment(field) => {
                let (env, issues) = environment_data::parse(
                    &field.read(cx).text,
                    self.peer_is_windows(&dialog.cycle),
                );
                if !issues.is_empty() {
                    return;
                }
                Mutation::Environment(env)
            }
            Editor::Delete => Mutation::Delete,
        };
        self.mutate_peer_session(dialog.cycle.clone(), dialog.id, action, window, cx);
    }

    fn peer_is_windows(&self, cycle: &source::PeerCycle) -> bool {
        self.peers
            .peers
            .get(&cycle.peer)
            .and_then(|state| state.snapshot.as_ref())
            .is_some_and(|snapshot| snapshot.probe.platform == "win32")
    }

    pub(crate) fn peer_environment_is_open(&self) -> bool {
        matches!(&self.dialog, Some(Dialog::PeerSession(dialog)) if matches!(dialog.editor, Editor::Environment(_)))
    }

    pub(crate) fn render_peer_session_environment(
        &self,
        dialog: &PeerSessionDialog,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Editor::Environment(text) = &dialog.editor else {
            return div().into_any_element();
        };
        let Some(session) = self.peer_session(&dialog.cycle, dialog.id) else {
            return div().into_any_element();
        };
        self.render_session_environment(
            session.title.clone(),
            text,
            self.peer_is_windows(&dialog.cycle),
            palette,
            window,
            cx,
        )
    }

    pub(crate) fn render_peer_session_delete(
        &self,
        palette: Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let Some(Dialog::PeerSession(dialog)) = &self.dialog else {
            return None;
        };
        if !matches!(dialog.editor, Editor::Delete) {
            return None;
        }
        let session = self.peer_session(&dialog.cycle, dialog.id)?;
        let running = session.tabs.iter().any(|tab| {
            matches!(tab.terminal_kind, Some(shellfox_protocol::TerminalKind::Embedded))
                && !matches!(tab.lifecycle, TabLifecycle::Closed)
        });
        Some(self.inline_session_delete(session.title.clone(), running, palette, cx))
    }

    pub(crate) fn dismiss_peer_session_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Dialog::PeerSession(dialog)) = self.dialog.take() {
            if let Some(prior) = dialog.prior {
                prior.focus(window);
            } else {
                self.focus_selected(window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn dismiss_completed_peer_dialog(
        &mut self,
        cycle: &source::PeerCycle,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(&self.dialog, Some(Dialog::PeerSession(dialog)) if dialog.cycle == *cycle && dialog.id == id)
        {
            self.dismiss_peer_session_dialog(window, cx);
        }
    }
}
