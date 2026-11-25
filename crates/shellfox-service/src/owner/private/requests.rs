use super::*;

pub(super) struct RequestContext {
    pub(super) client_id: uuid::Uuid,
    pub(super) epoch: Option<uuid::Uuid>,
    pub(super) cancel: shellfox_protocol::CancellationToken,
}
impl PrivateOwner {
    fn admit_attached_provider(
        &self,
        service: &mut Service,
        controller: u64,
        epoch: Option<uuid::Uuid>,
        provider: crate::local::DesktopProviderRequest,
    ) -> Result<crate::native::NativeAction> {
        anyhow::ensure!(
            epoch.is_some() && self.admits_attached_provider(provider.renderer),
            "Desktop providers require the current registered attached desktop."
        );
        let action = provider.into_native()?;
        match &action {
            crate::native::NativeAction::System(_) => service
                .admit_system_native(controller)
                .map_err(|error| anyhow::anyhow!(error.message))?,
            crate::native::NativeAction::Ssh(_) => {}
            _ => anyhow::bail!("Unsupported desktop provider request"),
        }
        Ok(action)
    }

    pub(super) fn handle_request(
        &mut self,
        service: &mut Service,
        controllers: &mut HashMap<uuid::Uuid, u64>,
        request: PrivateRequest,
        caller: RequestContext,
        reply: oneshot::Sender<Result<Value>>,
    ) {
        let RequestContext {
            client_id,
            epoch,
            cancel,
        } = caller;
        if epoch.is_some_and(|epoch| self.lifetimes.get(&client_id).map(|l| l.epoch) != Some(epoch))
        {
            let _ = reply.send(Err(anyhow::anyhow!(
                "Private client lifetime epoch is stale or closed."
            )));
            return;
        }
        let controller = match service
            .assert_owned()
            .and_then(|()| controller_id(controllers, service, client_id))
        {
            Ok(controller) => controller,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        match request {
            PrivateRequest::RemoteOperator(_) => {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "Remote operator requires its original asynchronous route"
                )));
            }
            PrivateRequest::Peer(_) => {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "Outgoing requires the original asynchronous lifetime route"
                )));
            }
            PrivateRequest::SshRead(request) => {
                let result = service.assert_accepting().and_then(|()| {
                    anyhow::ensure!(
                        epoch.is_some(),
                        "An authenticated SSH client lifetime is required."
                    );
                    service.read_ssh_settings(request).and_then(encode)
                });
                let _ = reply.send(result);
            }
            PrivateRequest::Power(request) => service.request_power(
                request,
                cancel,
                crate::power::service::PowerReply::Local(reply),
            ),
            PrivateRequest::DesktopView(request) => {
                let action = match *request {
                    crate::local::DesktopRequest::Provider(provider) => {
                        self.admit_attached_provider(service, controller, epoch, provider)
                    }
                    request => crate::native::NativeAction::try_from(request),
                };
                let action = match action {
                    Ok(action) => action,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return;
                    }
                };
                if let Some(target) = action.close_target() {
                    let admitted = action.validate().and_then(|()| {
                        anyhow::ensure!(
                            controller != 0,
                            "A native controller identity is required."
                        );
                        Ok(())
                    });
                    service.close_request(
                        target,
                        admitted,
                        Box::new(move |result| {
                            let _ = reply.send(
                                result
                                    .and_then(|snapshot| {
                                        crate::local::DesktopReply::try_from(
                                            crate::native::NativeResponse::Snapshot(snapshot),
                                        )
                                    })
                                    .and_then(encode),
                            );
                        }),
                    );
                    return;
                }
                if matches!(action, crate::native::NativeAction::TabStats) {
                    service.enqueue_tab_stats(crate::tab_stats::StatsReply::Desktop(reply));
                    return;
                }
                if matches!(action, crate::native::NativeAction::TerminalProfiles) {
                    service.enqueue_desktop_catalog(controller, reply);
                    return;
                }
                let result = service
                    .with_grant_recipient(cancel, |service| {
                        service.execute_attached_desktop(action, controller)
                    })
                    .and_then(crate::local::DesktopReply::try_from)
                    .and_then(encode);
                let _ = reply.send(result);
            }
            PrivateRequest::LocalApi(request) => {
                if let Some(target) = request.close_target() {
                    let admitted = shellfox_protocol::Validate::validate(&request)
                        .map_err(anyhow::Error::from);
                    service.close_request(
                        target,
                        admitted,
                        Box::new(move |result| {
                            let _ =
                                reply.send(result.map(LocalApiResponse::Snapshot).and_then(encode));
                        }),
                    );
                    return;
                }
                let detach = matches!(request, LocalApiRequest::Detach {});
                let result = service
                    .with_grant_recipient(cancel, |service| service.execute(request, controller));
                if detach && result.is_ok() {
                    controllers.remove(&client_id);
                }
                service.reply_after_launch(
                    result,
                    Box::new(move |result| drop(reply.send(result.and_then(encode)))),
                );
            }
            PrivateRequest::DesktopLaunch(request) if request.is_attached_renderer() => {
                let result = self
                    .attached_launch(service, client_id, epoch, request)
                    .and_then(encode);
                let _ = reply.send(result);
            }
            PrivateRequest::DesktopLaunch(request) => {
                let result = if matches!(request, DesktopLaunchRequest::Submit { .. }) {
                    service.assert_accepting()
                } else {
                    service.assert_owned()
                };
                let result = result
                    .and_then(|()| {
                        self.launches
                            .handle_local(request, service)
                            .map_err(anyhow::Error::from)
                    })
                    .and_then(encode);
                let _ = reply.send(result);
            }
            request => service.dispatch_automation(request, controller, reply, &mut self.launches),
        }
    }
}
