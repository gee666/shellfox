use super::*;
use crate::workspace::AfterAction;
use shellfox_host::{CreateSession, HostRequest, ProfileId};
use shellfox_service::native::NativeAction;
#[path = "directory_picker.rs"]
mod picker;

fn directory_profile<'a>(
    profiles: &'a [crate::settings::api::Profile],
    configured: &str,
    cwd: &str,
    windows: bool,
) -> Option<&'a str> {
    let ordinary = |profile: &&crate::settings::api::Profile| !profile.elevation;
    let configured_profile = profiles
        .iter()
        .filter(ordinary)
        .find(|profile| profile.id == configured);
    if !windows {
        return configured_profile
            .filter(|profile| profile.available)
            .map(|profile| profile.id.as_str());
    }
    let guest = [r"\\wsl$\", r"\\wsl.localhost\"].iter().find_map(|prefix| {
        cwd.get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| cwd[prefix.len()..].split('\\').next().unwrap_or_default())
    });
    if let Some(distro) = guest {
        return profiles
            .iter()
            .find(|profile| {
                profile.available
                    && profile
                        .id
                        .strip_prefix("wsl:")
                        .is_some_and(|name| name.eq_ignore_ascii_case(distro))
            })
            .map(|profile| profile.id.as_str());
    }
    if let Some(configured) = configured_profile.filter(|profile| {
        profile.id != "wsl" && !profile.id.starts_with("wsl:")
    }) {
        return configured.available.then_some(configured.id.as_str());
    }
    ["pwsh", "windows-powershell"]
        .into_iter()
        .find_map(|id| {
            profiles
                .iter()
                .find(|profile| profile.available && profile.id == id)
        })
        .or_else(|| {
            profiles.iter().filter(ordinary).find(|profile| {
                profile.available && profile.id != "wsl" && !profile.id.starts_with("wsl:")
            })
        })
        .map(|profile| profile.id.as_str())
}

impl Root {
    pub(crate) fn profile_for_directory(&self, cwd: &str) -> Option<&str> {
        directory_profile(&self.profiles, &self.default_profile(), cwd, cfg!(windows))
    }

    pub(crate) fn submit_directory(
        &mut self,
        mode: Option<Busy>,
        browsed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if draft.busy.is_some() || draft.seen.1 || self.user_action_pending {
            return;
        }
        let home = draft.home.clone();
        let target = draft.target.clone();
        if home.is_none() && target.is_none() {
            return;
        }
        let picked = browsed.is_some();
        let (mode, query) = match (mode, browsed, &draft.missing) {
            (Some(Busy::Creating), _, Some(missing)) => (Busy::Creating, missing.clone()),
            (_, Some(path), _) => (Busy::Checking, path),
            (_, None, None) => (Busy::Checking, draft.seen.0.clone()),
            _ => return,
        };
        let id = draft.id;
        draft.busy = Some(mode);
        if picked {
            draft.missing = None;
        }
        draft.completion.dismiss();
        draft.error = None;
        cx.spawn_in(window, async move |root, cx| {
            let result = if let Some(target) = &target {
                target
                    .resolve(query.clone(), mode == Busy::Creating)
                    .await
                    .map_err(|error| (error, query))
            } else if let Some(home) = home {
                cx.background_executor()
                    .spawn(async move {
                        paths::resolve(&home, &query, mode == Busy::Creating)
                            .map_err(|error| (error, query))
                    })
                    .await
            } else {
                return;
            };
            match result {
                Ok(cwd) if target.is_some() => {
                    let _ = root.update_in(cx, |root, window, cx| {
                        if let Some(target) = target {
                            root.create_peer_directory_session(cwd, target, id, window, cx);
                        }
                    });
                }
                Ok(cwd) => loop {
                    // A periodic snapshot request may still own the host slot. The form stays locked until it is free.
                    let proceed = root
                        .update_in(cx, |root, window, cx| {
                            let Some(Dialog::Create(draft)) = &mut root.dialog else {
                                return true;
                            };
                            if draft.id != id || root.closing_window {
                                return true;
                            }
                            if root.action_pending {
                                return false;
                            }
                            draft.busy = None;
                            draft.missing = None;
                            draft.path.read(cx).focus(window);
                            root.create_resolved_directory(cwd.clone(), window, cx);
                            true
                        })
                        .unwrap_or(true);
                    if proceed {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                },
                Err((error, query)) => {
                    let _ = root.update_in(cx, |root, window, cx| {
                        let Some(Dialog::Create(draft)) = &mut root.dialog else {
                            return;
                        };
                        if draft.id != id {
                            return;
                        }
                        draft.busy = None;
                        draft.missing = None;
                        if error.missing && mode != Busy::Creating && !picked {
                            draft.missing = Some(query);
                            draft.keep_editing.focus(window);
                        } else {
                            draft.error = Some(if picked {
                                "Choose an accessible local directory".into()
                            } else {
                                error.message
                            });
                            draft.path.read(cx).focus(window);
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn create_resolved_directory(
        &mut self,
        cwd: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile_id) =
            self.profile_for_directory(&cwd)
        else {
            self.error = Some("No discovered interactive terminal profile is available.".into());
            cx.notify();
            return;
        };
        match ProfileId::new(profile_id) {
            Ok(profile_id) => {
                let title = peer::session_title(&cwd);
                self.submit_native(
                    NativeAction::Metadata(HostRequest::CreateSession(CreateSession {
                        request_id: SessionId::new(),
                        title: Some(title),
                        cwd,
                        profile_id,
                    })),
                    AfterAction::Create,
                    window,
                    cx,
                );
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub(crate) fn browse_directory(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if draft.busy.is_some() || self.user_action_pending || draft.target.is_some() {
            return;
        }
        let id = draft.id;
        draft.busy = Some(Busy::Checking);
        draft.completion.dismiss();
        draft.error = None;
        let prompt = picker::choose(window, cx);
        cx.spawn_in(window, async move |root, cx| {
            let result = prompt.await;
            let _ = root.update_in(cx, |root, window, cx| {
                let Some(Dialog::Create(draft)) = &mut root.dialog else {
                    return;
                };
                if draft.id != id {
                    return;
                }
                draft.busy = None;
                match result {
                    Ok(Some(path)) => {
                        if let Some(path) = path.to_str() {
                            cx.notify();
                            root.submit_directory(None, Some(path.to_owned()), window, cx);
                            return;
                        }
                        draft.error = Some("Choose an accessible local directory".into());
                    }
                    Ok(None) => {}
                    Err(_) => draft.error = Some("Choose an accessible local directory".into()),
                }
                // Canceling the picker doesn't change the path awaiting confirmation.
                if draft.missing.is_some() {
                    draft.keep_editing.focus(window);
                } else {
                    draft.path.read(cx).focus(window);
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
    use super::*;
    use crate::settings::api::Profile;

    #[test]
    fn windows_falls_back_to_local_profiles_but_not_for_wsl_paths() {
        let mut profiles = ["pwsh", "windows-powershell", "wsl", "wsl:Debian"].map(|id| Profile {
            id: id.into(),
            label: id.into(),
            available: id != "wsl",
            unavailable_reason: None,
            elevation: false,
        });
        assert_eq!(
            directory_profile(&profiles, "wsl", r"C:\demo", true),
            Some("pwsh")
        );
        assert_eq!(
            directory_profile(&profiles, "windows-powershell", r"C:\demo", true),
            Some("windows-powershell")
        );
        assert_eq!(
            directory_profile(&profiles, "pwsh", r"\\WSL.LOCALHOST\Ubuntu\work", true),
            None
        );
        assert_eq!(
            directory_profile(&profiles, "wsl:Debian", r"C:\demo", true),
            Some("pwsh")
        );
        assert_eq!(
            directory_profile(&profiles, "pwsh", r"\\wsl$\debian\work", true),
            Some("wsl:Debian")
        );
        assert_eq!(
            directory_profile(&profiles, "missing", "/demo", false),
            None
        );
        profiles[0].available = false;
        assert_eq!(directory_profile(&profiles, "pwsh", r"C:\demo", true), None);
        assert_eq!(directory_profile(&profiles, "wsl:Debian", r"C:\demo", true), Some("windows-powershell"));
    }
}
