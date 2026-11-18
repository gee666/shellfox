use super::{acquire_linux_inhibitors, PowerError, PowerLease};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedFd,
};

pub struct Lease {
    session: Option<(Connection, u32)>,
    // The open descriptor owns the logind inhibitor. Closing it releases the request.
    inhibitor: Option<OwnedFd>,
    lid: Option<OwnedFd>,
    partial_reason: Option<String>,
}
fn screensaver(connection: &Connection) -> Result<Proxy<'_>, PowerError> {
    Proxy::new(
        connection,
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    )
    .map_err(|e| PowerError::operation("ScreenSaver proxy", e))
}
fn acquire_display() -> Result<(Connection, u32), PowerError> {
    let session = Connection::session().map_err(|e| PowerError::operation("session bus", e))?;
    let cookie: u32 = screensaver(&session)?
        .call("Inhibit", &("org.shellfox.Shellfox", "Keep awake"))
        .map_err(|e| PowerError::operation("ScreenSaver Inhibit", e))?;
    Ok((session, cookie))
}
pub fn acquire() -> Result<Lease, PowerError> {
    let mut reasons = Vec::new();
    // Headless hosts can use logind without a desktop session bus. Keep each
    // successful request, rather than attempting an unconfirmed rollback.
    let session = match acquire_display() {
        Ok(session) => Some(session),
        Err(error) => {
            reasons.push(format!("Display inhibitor unavailable: {error}"));
            None
        }
    };
    let mut lease = Lease {
        session,
        inhibitor: None,
        lid: None,
        partial_reason: None,
    };
    let requests = (|| {
        let system = Connection::system().map_err(|e| PowerError::operation("system bus", e))?;
        let login = Proxy::new(
            &system,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .map_err(|e| PowerError::operation("logind proxy", e))?;
        acquire_linux_inhibitors(|what| {
            login
                .call::<_, _, OwnedFd>("Inhibit", &(what, "Shellfox", "Keep awake", "block"))
                .map_err(|error| PowerError::operation("logind Inhibit", error))
        })
    })();
    match requests {
        Ok(requests) => {
            lease.inhibitor = Some(requests.primary);
            lease.lid = requests.lid;
            if let Some(reason) = requests.partial_reason {
                reasons.push(reason);
            }
        }
        Err(error) => reasons.push(format!("System/lid inhibitors unavailable: {error}")),
    }
    if lease.session.is_none() && lease.inhibitor.is_none() {
        return Err(PowerError(reasons.join("; ")));
    }
    lease.partial_reason = (!reasons.is_empty()).then(|| reasons.join("; "));
    Ok(lease)
}
impl PowerLease for Lease {
    fn release(&mut self) -> Result<(), PowerError> {
        if let Some((session, cookie)) = &self.session {
            screensaver(session)?
                .call::<_, _, ()>("UnInhibit", &(*cookie,))
                .map_err(|e| PowerError::operation("ScreenSaver UnInhibit", e))?;
            self.session = None;
        }
        self.inhibitor = None;
        self.lid = None;
        Ok(())
    }
    fn partial_reason(&self) -> Option<&str> {
        self.partial_reason.as_deref()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
