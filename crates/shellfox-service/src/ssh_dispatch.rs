use super::*;
impl SshManager {
    pub fn request(
        &mut self,
        controller: u64,
        request: Request,
        validator: &dyn NativeControllerValidator,
        starter: &mut dyn OwnedRuntimeStarter,
    ) -> Result<Response> {
        self.check(controller, None, validator)?;
        request.validate()?;
        self.poll(validator, starter);
        let check = || {
            self.check(controller, None, validator)
                .map_err(|_| shellfox_ssh::Error::Permissions)
        };
        match request {
            Request::List => Ok(Response::Profiles(self.profiles.list()?)),
            Request::Save(input) => Ok(Response::Profiles(
                self.profiles.save_guarded(input.into_profile(), check)?,
            )),
            Request::Delete { id } => {
                Ok(Response::Profiles(self.profiles.delete_guarded(id, check)?))
            }
            Request::Import => {
                let sessions = self
                    .platform
                    .import_sessions()
                    .map_err(|_| anyhow::anyhow!("Native SSH import failed."))?;
                Ok(Response::Imported(
                    self.profiles.import_guarded(sessions, check)?,
                ))
            }
            Request::ValidateKey { selected } => {
                let bytes = self.directory.read_key_file(selected.path())?;
                let metadata = shellfox_ssh::key_metadata::inspect(&bytes)?;
                let cryptographically_validated =
                    match shellfox_ssh::keys::parse_private_key(&bytes, None) {
                        Ok(_) => true,
                        Err(shellfox_ssh::Error::PassphraseRequired) => false,
                        Err(shellfox_ssh::Error::UnsupportedKeyFormat)
                            if metadata.limitation.is_some() =>
                        {
                            false
                        }
                        Err(shellfox_ssh::Error::InvalidKey)
                            if metadata.encrypted && metadata.limitation.is_some() =>
                        {
                            false
                        }
                        Err(error) => return Err(error.into()),
                    };
                self.check(controller, None, validator)?;
                let path = selected
                    .path()
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Unsupported SSH key path encoding."))?
                    .to_owned();
                Ok(Response::SelectedKey(SelectedKeyInfo {
                    path,
                    metadata,
                    cryptographically_validated,
                }))
            }
            Request::GetStatus { connection } => Ok(Response::Status(
                self.visible_statuses(controller, connection, validator),
            )),
            Request::Connect {
                profile,
                tab,
                cols,
                rows,
            } => {
                self.check(controller, Some(tab), validator)?;
                self.connections.retain(|_, c| {
                    !matches!(c.status.state, State::Closed | State::Failed { .. })
                        || c.runtime.is_some()
                        || c.job.is_some()
                        || c.broker.is_some()
                        || c.identity.is_some()
                        || c.reserved
                });
                anyhow::ensure!(self.connections.len() < 64, "SSH connection limit reached.");
                anyhow::ensure!(
                    !self.connections.values().any(|c| c.status.tab == tab),
                    "This tab already has an SSH connection."
                );
                let profile = self
                    .profiles
                    .get_for_connection(profile)?
                    .ok_or_else(|| anyhow::anyhow!("SSH profile no longer exists."))?;
                self.check(controller, Some(tab), validator)?;
                let connection = Uuid::new_v4();
                let cancel = CancellationToken::new();
                let auth_cancel = CancellationToken::new();
                let target = SourceConnectTarget::new(connection, tab);
                let status = ConnectionStatus {
                    runtime: None,
                    connection,
                    profile: profile.id,
                    tab,
                    state: State::Preparing,
                };
                let work = Work {
                    connection,
                    events: self.events.clone(),
                    cancel: auth_cancel.clone(),
                    lost: self.lost.clone(),
                };
                let backend = self.backend.clone();
                let directory = self.directory.clone();
                let pty = RemotePty::new(cols, rows)?;
                starter.reserve_target(controller, target)?;
                self.connections.insert(
                    connection,
                    Connection {
                        controller,
                        status: status.clone(),
                        cancel,
                        auth_cancel,
                        authenticated: false,
                        view_attached: true,
                        target,
                        reserved: true,
                        job: None,
                        pending: None,
                        runtime: None,
                        broker: None,
                        identity: None,
                        proof: None,
                    },
                );
                if let Err(error) = self.check(controller, Some(tab), validator) {
                    self.owned(controller, connection)?.stop();
                    self.poll(validator, starter);
                    return Err(error);
                }
                let spawned = std::thread::Builder::new()
                    .name("shellfox-ssh-prepare".into())
                    .spawn(move || {
                        let result = work.prepare(&backend, directory, profile, pty);
                        let _ = work
                            .events
                            .try_send(WorkEvent::Prepared { connection, result });
                    });
                match spawned {
                    Ok(job) => self.owned(controller, connection)?.job = Some(job),
                    Err(_) => {
                        self.owned(controller, connection)?.stop();
                        self.poll(validator, starter);
                        anyhow::bail!("SSH preparation could not start.")
                    }
                }
                Ok(Response::Status(vec![status]))
            }
            Request::AdoptReady { connection } => {
                Ok(Response::Status(vec![self.adopt_ready(
                    controller, connection, validator, starter,
                )?]))
            }
            Request::ConfirmHost {
                connection,
                challenge,
                decision,
            } => {
                let tab = self.owned(controller, connection)?.status.tab;
                self.check(controller, Some(tab), validator)?;
                let c = self.owned(controller, connection)?;
                match c.pending.take() {
                    Some(Pending::Host(id, tx)) if id == challenge => {
                        tx.try_send(decision)
                            .map_err(|_| anyhow::anyhow!("SSH host challenge expired."))?;
                        c.status.state = State::Preparing;
                    }
                    pending => {
                        c.pending = pending;
                        anyhow::bail!("SSH host challenge is stale.")
                    }
                }
                Ok(Response::Applied)
            }
            Request::AnswerPrompt {
                connection,
                prompt,
                answer,
            } => {
                let tab = self.owned(controller, connection)?.status.tab;
                self.check(controller, Some(tab), validator)?;
                let c = self.owned(controller, connection)?;
                match c.pending.take() {
                    Some(Pending::Prompt(id, tx)) if id == prompt => {
                        tx.try_send(answer)
                            .map_err(|_| anyhow::anyhow!("SSH prompt expired."))?;
                        c.status.state = if c.authenticated {
                            State::Ready
                        } else if c.runtime.is_some() {
                            State::Authenticating
                        } else {
                            State::Preparing
                        };
                    }
                    pending => {
                        c.pending = pending;
                        anyhow::bail!("SSH prompt is stale.")
                    }
                }
                Ok(Response::Applied)
            }
            Request::Disconnect { connection } => {
                let c = self.owned(controller, connection)?;
                c.stop();
                self.poll(validator, starter);
                Ok(Response::Status(
                    self.statuses(controller, Some(connection)),
                ))
            }
        }
    }
}
