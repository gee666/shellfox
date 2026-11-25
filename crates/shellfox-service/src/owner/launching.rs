//! Create and addTab replies that wait for their new shell, as 0.9 replied
//! only once it was open. The owner keeps serving other requests meanwhile.
use crate::{local_api::LocalApiResponse, Service};
use anyhow::Result;

/// Sends one local API reply, on whichever route the request came.
pub(crate) type LaunchReply = Box<dyn FnOnce(Result<LocalApiResponse>) + Send>;

/// A tab created through the local API whose shell failed to start: 0.9's
/// LAUNCH_FAILED. The saved tab stays, as in 0.9.
#[derive(Debug)]
pub struct LaunchFailure(pub String);
impl std::fmt::Display for LaunchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LaunchFailure {}

pub(crate) fn failure(error: &anyhow::Error) -> anyhow::Error {
    let cause = error.root_cause().to_string();
    anyhow::Error::new(LaunchFailure(format!(
        "The shell could not be started: {cause}"
    )))
}

impl Service {
    /// Replies at once, or once the start the request began has settled.
    pub(crate) fn reply_after_launch(
        &mut self,
        result: Result<LocalApiResponse>,
        reply: LaunchReply,
    ) {
        match self.launch_begun.take() {
            Some(tab) if result.is_ok() => self.launch_replies.push((tab, reply)),
            _ => reply(result),
        }
    }
    /// Answers each waiting reply whose start settled: the fresh snapshot, or
    /// the start's failure.
    pub(crate) fn finish_launches(&mut self) {
        let mut index = 0;
        while index < self.launch_replies.len() {
            if !self.start_settled(self.launch_replies[index].0) {
                index += 1;
                continue;
            }
            let (tab, reply) = self.launch_replies.swap_remove(index);
            let result = match self.start_terminal(tab) {
                Ok(()) => self
                    .host()
                    .snapshot()
                    .map(LocalApiResponse::Snapshot)
                    .map_err(Into::into),
                Err(error) => Err(failure(&error)),
            };
            reply(result);
        }
    }
}
