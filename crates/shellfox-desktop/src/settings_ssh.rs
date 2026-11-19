use super::*;
#[derive(Clone)]
pub(super) enum Operation {
    Add,
    Edit(String),
    Cancel,
    Save,
    Remove(String),
    ConfirmRemove(String),
    Import,
    ChooseKey,
    ClearKey,
    ClearPassword,
}
pub(super) struct Editor {
    pub id: String,
    pub is_new: bool,
    pub has_password: bool,
    pub clear_password: bool,
    pub name: Entity<Field>,
    pub host: Entity<Field>,
    pub port: Entity<Field>,
    pub user: Entity<Field>,
    pub password: Entity<Field>,
    pub key: Entity<Field>,
    pub cwd: Entity<Field>,
}
impl Editor {
    fn new(profile: Option<&api::SshProfile>, cx: &mut App) -> Self {
        let password = Field::password(cx);
        let mut input = |text| {
            let field = Field::settings_input(None, cx);
            field.update(cx, |field, cx| field.set_text(text, cx));
            field
        };
        let editor = Self {
            id: profile
                .map(|p| p.id.clone())
                .unwrap_or_else(|| shellfox_core::SessionId::new().to_string()),
            is_new: profile.is_none(),
            has_password: profile.is_some_and(|p| p.has_password),
            clear_password: false,
            name: input(profile.map(|p| p.name.clone()).unwrap_or_default()),
            host: input(profile.map(|p| p.host.clone()).unwrap_or_default()),
            port: input(profile.map(|p| p.port.to_string()).unwrap_or_default()),
            user: input(profile.map(|p| p.user.clone()).unwrap_or_default()),
            password,
            key: input(profile.and_then(|p| p.key_file.clone()).unwrap_or_default()),
            cwd: input(
                profile
                    .and_then(|p| p.remote_cwd.clone())
                    .unwrap_or_default(),
            ),
        };
        editor.configure_fields(false, cx);
        editor
    }
    pub(super) fn fields(&self) -> [Entity<Field>; 7] {
        [
            self.name.clone(),
            self.host.clone(),
            self.port.clone(),
            self.user.clone(),
            self.password.clone(),
            self.key.clone(),
            self.cwd.clone(),
        ]
    }
    pub(super) fn configure_fields(&self, pending: bool, cx: &mut App) {
        let saved = self.has_password && !self.clear_password;
        for (field, placeholder, read_only) in [
            (self.name.clone(), "Name", pending),
            (self.host.clone(), "Host", pending),
            (self.port.clone(), "22", pending),
            (self.user.clone(), "User", pending),
            (
                self.password.clone(),
                if saved {
                    "•••••• saved"
                } else {
                    "Password"
                },
                pending,
            ),
            (self.key.clone(), "Key file", true),
            (self.cwd.clone(), "~ (home)", pending),
        ] {
            field.update(cx, |field, cx| {
                field.configure(
                    crate::field::FieldOptions {
                        placeholder: Some(placeholder.into()),
                        read_only: Some(read_only),
                        ..Default::default()
                    },
                    cx,
                )
            });
        }
    }
}
#[derive(Default)]
pub(super) struct SshView {
    pub profiles: Option<Vec<api::SshProfile>>,
    pub editor: Option<Editor>,
    pub removing: Option<String>,
    pub pending: bool,
    pub reading: bool,
    pub epoch: u64,
    pub error: Option<String>,
    pub status: Option<String>,
    pub status_epoch: u64,
    pub focus_control: Option<String>,
    pub hovered_row: Option<String>,
    editor_subscriptions: Vec<Subscription>,
}
impl SshView {
    pub fn cancel_editor(&mut self) -> bool {
        if self.pending {
            return false;
        }
        if self.editor.is_some() || self.removing.is_some() {
            self.focus_control = Some(
                self.editor
                    .as_ref()
                    .filter(|editor| !editor.is_new)
                    .map(|editor| editor.id.as_str())
                    .or(self.removing.as_deref())
                    .map_or_else(|| "add-ssh".into(), row_id),
            );
            self.clear_editor();
            self.error = None;
            true
        } else {
            false
        }
    }
    pub fn clear_editor(&mut self) {
        self.editor_subscriptions.clear();
        self.editor = None;
        self.removing = None;
    }
}

pub(super) fn row_id(id: &str) -> String {
    format!("ssh-row-{id}")
}

impl SettingsPanel {
    pub(super) fn ssh_action(
        &mut self,
        operation: Operation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() {
            return;
        }
        if self.ssh.pending {
            return;
        }
        if self.ssh.profiles.is_none() && !matches!(operation, Operation::Cancel) {
            return;
        }
        match operation {
            Operation::Add | Operation::Edit(_) => {
                let Some(profiles) = &self.ssh.profiles else {
                    return;
                };
                let profile = match &operation {
                    Operation::Edit(id) => profiles.iter().find(|profile| profile.id == *id),
                    _ => None,
                };
                if matches!(operation, Operation::Edit(_)) && profile.is_none() {
                    self.ssh.error = Some("SSH profile is no longer available.".into());
                    cx.notify();
                    return;
                }
                let editor = Editor::new(profile, cx);
                self.ssh.clear_editor();
                for field in editor.fields() {
                    field.update(cx, |field, cx| field.apply_palette(self.palette, cx));
                    let mut seen = Arc::downgrade(&field.read(cx).snapshot_text());
                    self.ssh.editor_subscriptions.push(cx.observe_in(
                        &field,
                        window,
                        move |panel, field, _, cx| {
                            let current = Arc::downgrade(&field.read(cx).snapshot_text());
                            if !current.ptr_eq(&seen) {
                                seen = current;
                                if !panel.ssh.pending && panel.ssh.error.take().is_some() {
                                    cx.notify();
                                }
                            }
                        },
                    ));
                }
                editor.name.read(cx).focus(window);
                self.ssh.editor = Some(editor);
                self.ssh.removing = None;
                self.ssh.error = None;
                cx.notify();
                return;
            }
            Operation::Cancel => {
                self.ssh.cancel_editor();
                cx.notify();
                return;
            }
            Operation::Remove(id) => {
                self.ssh.clear_editor();
                self.ssh.focus_control = Some(format!("confirm-remove-{id}"));
                self.ssh.removing = Some(id);
                self.ssh.error = None;
                cx.notify();
                return;
            }
            Operation::ChooseKey => {
                self.choose_ssh_key(window, cx);
                return;
            }
            Operation::ClearKey => {
                if let Some(editor) = &self.ssh.editor {
                    editor
                        .key
                        .update(cx, |field, cx| field.set_text(String::new(), cx));
                    editor.key.read(cx).focus(window);
                }
                self.ssh.error = None;
                cx.notify();
                return;
            }
            Operation::ClearPassword => {
                if let Some(editor) = &mut self.ssh.editor {
                    editor.clear_password = true;
                    editor
                        .password
                        .update(cx, |field, cx| field.set_text(String::new(), cx));
                    editor.configure_fields(false, cx);
                    editor.password.read(cx).focus(window);
                }
                self.ssh.error = None;
                cx.notify();
                return;
            }
            _ => {}
        }
        let (request, after) = match operation {
            Operation::Save => match self.ssh_input(cx) {
                Ok(input) => (api::Request::SshSave(input), aux::After::Ssh),
                Err(error) => {
                    self.ssh.error = Some(error);
                    cx.notify();
                    return;
                }
            },
            Operation::ConfirmRemove(id) => (api::Request::SshDelete(id), aux::After::Ssh),
            Operation::Import => {
                if self.ssh.editor.is_some() || self.ssh.removing.is_some() {
                    return;
                }
                self.ssh.status = None;
                self.ssh.status_epoch += 1;
                (api::Request::SshImport, aux::After::Import)
            }
            _ => return,
        };
        self.ssh.pending = true;
        if let Some(editor) = &self.ssh.editor {
            editor.configure_fields(true, cx);
        }
        self.ssh.error = None;
        self.request_aux(request, after, cx);
    }
    fn ssh_input(&self, cx: &App) -> Result<api::SshInput, String> {
        let editor = self.ssh.editor.as_ref().ok_or("SSH editor is closed.")?;
        let name = editor.name.read(cx).text.trim().to_owned();
        let host = editor.host.read(cx).text.trim().to_owned();
        let port = editor.port.read(cx).text.trim().to_owned();
        let port = validate_endpoint(&name, &host, &port)?;
        let password = editor.password.read(cx).text.clone();
        let key = editor.key.read(cx).text.clone();
        let cwd = editor.cwd.read(cx).text.trim().to_owned();
        Ok(api::SshInput {
            id: editor.id.clone(),
            name,
            host,
            port,
            user: editor.user.read(cx).text.trim().into(),
            password: if !password.is_empty() {
                api::Password::Replace(password)
            } else if editor.clear_password {
                api::Password::Clear
            } else {
                api::Password::Preserve
            },
            key_file: if key.is_empty() { None } else { Some(key) },
            remote_cwd: if cwd.is_empty() { None } else { Some(cwd) },
        })
    }
}

fn validate_endpoint(name: &str, host: &str, port: &str) -> Result<u16, String> {
    let mut errors = Vec::new();
    if name.is_empty() {
        errors.push("Name is required.");
    }
    if host.is_empty() {
        errors.push("Host is required.");
    }
    let parsed = if port.is_empty() {
        Some(22)
    } else if port.bytes().all(|byte| byte.is_ascii_digit()) {
        port.parse::<u16>().ok().filter(|port| *port != 0)
    } else {
        None
    };
    if parsed.is_none() {
        errors.push("Port must be between 1 and 65535.");
    }
    parsed
        .filter(|_| errors.is_empty())
        .ok_or_else(|| errors.join(" "))
}

#[cfg(test)]
#[test]
fn endpoint_validation_matches_the_form() {
    assert_eq!(validate_endpoint("Fixture", "localhost", ""), Ok(22));
    assert_eq!(
        validate_endpoint("Fixture", "localhost", "065535"),
        Ok(65535)
    );
    for port in ["0", "65536", "+22", "22.0", "2e1"] {
        assert_eq!(
            validate_endpoint("", "", port),
            Err("Name is required. Host is required. Port must be between 1 and 65535.".into())
        );
    }
}
