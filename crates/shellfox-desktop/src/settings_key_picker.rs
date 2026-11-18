use super::*;
use std::path::PathBuf;
pub(super) fn selected(
    paths: Option<Vec<PathBuf>>,
) -> anyhow::Result<Option<shellfox_service::ssh::SelectedKey>> {
    match paths {
        None => Ok(None),
        Some(paths) => {
            anyhow::ensure!(paths.len() == 1, "Select exactly one SSH key file.");
            Ok(Some(shellfox_service::ssh::SelectedKey::new(
                paths
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("SSH key selection is empty."))?,
            )?))
        }
    }
}
impl SettingsPanel {
    pub(super) fn choose_ssh_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operations.frozen() || self.ssh.pending {
            return;
        }
        let Some(editor) = &self.ssh.editor else {
            return;
        };
        let editor_id = editor.name.entity_id();
        editor.configure_fields(true, cx);
        self.ssh.pending = true;
        self.ssh.error = None;
        let lease = self.operations.begin();
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose SSH key file".into()),
        });
        cx.spawn_in(window, async move |panel, cx| {
            let result = prompt.await;
            let _ = panel.update_in(cx, |panel, _, cx| {
                drop(lease);
                let current = panel
                    .ssh
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.name.entity_id() == editor_id);
                if !current || !panel.visible {
                    panel.ssh.pending = false;
                    if let Some(editor) = &panel.ssh.editor {
                        editor.configure_fields(panel.operations.frozen(), cx);
                    }
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(paths)) => match selected(paths) {
                        Ok(Some(selected)) => panel.request_aux(
                            api::Request::ValidateSshKey(selected),
                            aux::After::Key,
                            cx,
                        ),
                        Ok(None) => {
                            panel.ssh.pending = false;
                            if let Some(editor) = &panel.ssh.editor {
                                editor.configure_fields(false, cx);
                            }
                        }
                        Err(error) => {
                            panel.ssh.pending = false;
                            panel.ssh.error = Some(error.to_string());
                            if let Some(editor) = &panel.ssh.editor {
                                editor.configure_fields(false, cx);
                            }
                        }
                    },
                    _ => {
                        panel.ssh.pending = false;
                        panel.ssh.error = Some("SSH key file picker could not complete.".into());
                        if let Some(editor) = &panel.ssh.editor {
                            editor.configure_fields(false, cx);
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
    use super::selected;
    #[test]
    fn cancellation_has_no_validation_or_rpc_intent() {
        assert!(selected(None).unwrap().is_none());
    }
    #[test]
    fn invalid_or_multiple_selections_are_rejected_before_native_admission() {
        assert!(selected(Some(vec![])).is_err());
        assert!(selected(Some(vec!["relative-key".into()])).is_err());
        assert!(selected(Some(vec!["/first".into(), "/second".into()])).is_err());
    }
}
