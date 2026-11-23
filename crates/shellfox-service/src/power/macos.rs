use super::{PowerError, PowerLease};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

pub struct Lease(Option<Child>);

fn arguments(owner: u32) -> [String; 3] {
    ["-dimsu".into(), "-w".into(), owner.to_string()]
}

pub fn acquire() -> Result<Lease, PowerError> {
    let child = Command::new("/usr/bin/caffeinate")
        .args(arguments(std::process::id()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PowerError::operation("Start caffeinate", error))?;
    let mut lease = Lease(Some(child));
    // caffeinate has no readiness pipe. Check for a rejected launch before
    // publishing success; -w also releases its assertions on owner exit.
    std::thread::sleep(Duration::from_millis(300));
    let status = lease
        .0
        .as_mut()
        .map(Child::try_wait)
        .transpose()
        .map_err(|error| PowerError::operation("Check caffeinate", error))?
        .flatten();
    if let Some(status) = status {
        lease.0 = None;
        return Err(PowerError::operation(
            "caffeinate exited before acquiring assertions",
            status,
        ));
    }
    Ok(lease)
}

impl PowerLease for Lease {
    fn release(&mut self) -> Result<(), PowerError> {
        if let Some(child) = &mut self.0 {
            let stopped = match child
                .try_wait()
                .map_err(|error| PowerError::operation("Check caffeinate", error))?
            {
                Some(_) => Ok(()),
                None => child.kill(),
            };
            if let Err(error) = stopped {
                if child
                    .try_wait()
                    .map_err(|error| PowerError::operation("Check caffeinate", error))?
                    .is_none()
                {
                    return Err(PowerError::operation("Stop caffeinate", error));
                }
            }
            child
                .wait()
                .map_err(|error| PowerError::operation("Join caffeinate", error))?;
            self.0 = None;
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn caffeinate_covers_the_same_assertions_and_owner_as_electron() {
        assert_eq!(super::arguments(1234), ["-dimsu", "-w", "1234"]);
    }
}
