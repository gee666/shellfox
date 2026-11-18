use super::*;
use crate::{desktop_launch::DesktopLaunchRouter, local::PrivateRequest};
impl Service {
    pub(crate) fn dispatch_automation(
        &mut self,
        request: PrivateRequest,
        controller: u64,
        reply: JsonReply,
        launches: &mut DesktopLaunchRouter,
    ) {
        match request {
            PrivateRequest::AutomationWrite(request) => {
                let result = self.assert_accepting().and_then(|()| {
                    if self.automation.writes.len() >= MAX_DEFERRED {
                        bail!("Automation write receipt bound reached; write was not admitted.");
                    }
                    self.automation.runs.begin_write(request, controller)
                });
                match result {
                    Ok(receipt) => self.automation.writes.push(PendingWrite { receipt, reply }),
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            PrivateRequest::AutomationStop(request) => {
                let id = request.id;
                let result = self.assert_owned().and_then(|()| {
                    if self.automation.stops.len() >= MAX_DEFERRED {
                        bail!("Automation stop receipt bound reached; stop was not admitted.");
                    }
                    self.begin_automation_stop(request)
                });
                match result {
                    Ok(()) => self.automation.stops.push(PendingStop { id, reply }),
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            request => {
                let result = match request {
                    PrivateRequest::AutomationStart(input) => {
                        self.start_automation(*input, controller).and_then(encode)
                    }
                    PrivateRequest::AutomationList(_) => {
                        self.automation.runs.list().and_then(encode)
                    }
                    PrivateRequest::AutomationRead(input) => self
                        .automation
                        .runs
                        .read(input, controller)
                        .and_then(encode),
                    PrivateRequest::AutomationShow(input) => self
                        .assert_accepting()
                        .and_then(|()| self.show_automation(input, launches))
                        .and_then(encode),
                    PrivateRequest::Power(_)
                    | PrivateRequest::OutgoingV2(_)
                    | PrivateRequest::RemoteOperator(_)
                    | PrivateRequest::Experiment(_)
                    | PrivateRequest::DesktopLaunch(_)
                    | PrivateRequest::AutomationWrite(_)
                    | PrivateRequest::AutomationStop(_) => Err(anyhow::anyhow!(
                        "Unexpected private owner automation route."
                    )),
                };
                let _ = reply.send(result);
            }
        }
    }
}
fn encode(value: impl serde::Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}
