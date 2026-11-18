use super::*;

impl Root {
    pub(crate) fn poll_ssh(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(target) = &mut self.ssh_ui.target else {
            return;
        };
        if target.intent != self.ssh_ui.latest
            || target.busy
            || target.ready_attempted
            || self.closing_window
            || self.navigation.epoch() != target.epoch
            || self.navigation.selected().tab_id != Some(target.tab)
        {
            return;
        }
        target.busy = true;
        let tab = target.tab;
        let epoch = target.epoch;
        let prior = target.status.clone();
        let client = target.client.clone();
        let has_view = self.views.contains_key(&tab);
        let visible = !self.settings && self.dialog.is_none();
        let latest = target.intent;
        cx.spawn_in(window, async move |root, cx| {
            let result: anyhow::Result<(ConnectionStatus, Option<Attachment>)> = async {
                let NativeResponse::Ssh(ssh::Response::Status(rows)) = client
                    .execute(NativeAction::Ssh(ssh::Request::GetStatus {
                        connection: prior.as_ref().map(|status| status.connection),
                    }))
                    .await?
                else {
                    anyhow::bail!("Invalid SSH status response");
                };
                let mut matches = rows.into_iter().filter(|status| {
                    status.tab == tab
                        && prior.as_ref().is_none_or(|old| {
                            old.connection == status.connection && old.profile == status.profile
                        })
                });
                let status = matches.next().ok_or_else(|| {
                    anyhow::anyhow!("SSH status is unavailable in this window's native scope")
                })?;
                anyhow::ensure!(matches.next().is_none(), "Ambiguous SSH target status");
                let runtime = status.runtime;
                if !visible {
                    return Ok((status, None));
                }
                if matches!(status.state, State::Ready) {
                    let NativeResponse::Ssh(ssh::Response::Status(adopted)) = client
                        .execute(NativeAction::Ssh(ssh::Request::AdoptReady {
                            connection: status.connection,
                        }))
                        .await?
                    else {
                        anyhow::bail!("Invalid SSH adoption response");
                    };
                    anyhow::ensure!(
                        adopted.len() == 1
                            && adopted[0].connection == status.connection
                            && adopted[0].tab == tab
                            && adopted[0].runtime == runtime
                            && matches!(adopted[0].state, State::Ready),
                        "SSH adoption target changed"
                    );
                    let expected = runtime.ok_or_else(|| {
                        anyhow::anyhow!("Ready SSH runtime identity is unavailable")
                    })?;
                    let attachment = client.attach(tab, false).await?;
                    if attachment.generation != expected.generation || expected.tab_id != tab {
                        client.detach(tab, attachment).await?;
                        anyhow::bail!("SSH control generation changed");
                    }
                    Ok((status, Some(Attachment::Control(attachment))))
                } else if let Some(expected) = runtime.filter(|_| !has_view) {
                    let Ok(NativeResponse::Observed(attachment)) =
                        client.execute(NativeAction::Observe { tab }).await
                    else {
                        return Ok((status, None));
                    };
                    if attachment.generation != expected.generation || expected.tab_id != tab {
                        client
                            .execute(NativeAction::Unobserve { tab, attachment })
                            .await?;
                        anyhow::bail!("SSH observer generation changed");
                    }
                    Ok((status, Some(Attachment::Observer(attachment))))
                } else {
                    Ok((status, None))
                }
            }
            .await;
            let _ = root.update_in(cx, |root, window, cx| {
                let current = root.ssh_ui.latest == latest
                    && root
                        .ssh_ui
                        .target
                        .as_ref()
                        .is_some_and(|target| target.tab == tab && target.epoch == epoch)
                    && root.navigation.epoch() == epoch
                    && root.selected_tab() == Some(tab)
                    && !root.closing_window
                    && (!visible || !root.settings && root.dialog.is_none());
                if let Some(target) = &mut root.ssh_ui.target
                    && target.tab == tab
                    && target.epoch == epoch
                {
                    target.busy = false;
                }
                match result {
                    Ok((status, attachment)) => {
                        if !current {
                            if let Some(attachment) = attachment {
                                release(client.clone(), tab, attachment, cx);
                            }
                            return;
                        }
                        let ready = matches!(status.state, State::Ready);
                        let admitted_control = ready && attachment.is_some();
                        let finished = matches!(status.state, State::Closed | State::Failed { .. });
                        if status.runtime.is_some_and(|identity| {
                            root.snapshot.as_ref().is_none_or(|snapshot| {
                                !snapshot.sessions.iter().any(|session| {
                                    session.id == identity.session_id
                                        && session.tabs.iter().any(|item| item.id == tab)
                                })
                            })
                        }) {
                            if let Some(attachment) = attachment {
                                release(client.clone(), tab, attachment, cx);
                            }
                            root.error = Some("SSH runtime session identity changed".into());
                            cx.notify();
                            return;
                        }
                        if let Some(target) = &mut root.ssh_ui.target {
                            target.status = Some(status);
                            target.ready_attempted = admitted_control || finished;
                        }
                        if root.views.contains_key(&tab) && !ready {
                            if let Some(attachment) = attachment {
                                release(client.clone(), tab, attachment, cx);
                            }
                            return;
                        }
                        if let Some(attachment) = attachment {
                            let opened = match attachment.clone() {
                                Attachment::Control(attachment) => TerminalView::open_attached(
                                    client.clone(),
                                    tab,
                                    attachment,
                                    window,
                                    cx,
                                ),
                                Attachment::Observer(attachment) => TerminalView::open_observed(
                                    client.clone(),
                                    tab,
                                    attachment,
                                    window,
                                    cx,
                                ),
                            };
                            match opened {
                                Ok(view) => {
                                    let marker = root
                                        .snapshot
                                        .as_ref()
                                        .and_then(|snapshot| {
                                            snapshot.sessions.iter().find(|session| {
                                                session.tabs.iter().any(|item| item.id == tab)
                                            })
                                        })
                                        .map(|session| session.settled_at.clone());
                                    if let Some(marker) = marker {
                                        view.update(cx, |view, cx| {
                                            view.set_captured_settlement(marker, cx)
                                        });
                                        root.views.insert(tab, view);
                                        root.apply_terminal_preferences(window, cx);
                                    }
                                }
                                Err(error) => {
                                    release(client.clone(), tab, attachment, cx);
                                    root.error = Some(error.message);
                                }
                            }
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        if current {
                            if let Some(target) = &mut root.ssh_ui.target {
                                target.ready_attempted = true;
                            }
                            root.error = Some(format!("SSH presentation unavailable: {error}"));
                            cx.notify();
                        }
                    }
                }
            });
        })
        .detach();
    }
}
#[derive(Clone)]
enum Attachment {
    Control(shellfox_service::Attachment),
    Observer(shellfox_service::native::NativeObserverAttachment),
}
fn release(client: HostClient, tab: TabId, attachment: Attachment, cx: &mut Context<Root>) {
    cx.spawn(async move |_, _| match attachment {
        Attachment::Control(attachment) => {
            let _ = client.detach(tab, attachment).await;
        }
        Attachment::Observer(attachment) => {
            let _ = client
                .execute(NativeAction::Unobserve { tab, attachment })
                .await;
        }
    })
    .detach();
}
