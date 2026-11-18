use super::*;
use shellfox_core::CreateSession;
use shellfox_host::CreationRequestReceipt;

/// Native system adapter checks existence/access and local-drive/WSL policy.
/// Return the normalized path spelling. The service rechecks it even when the
/// invoking process already validated cwd.
pub trait LaunchDirectoryPolicy {
    fn validate_directory(&self, cwd: &str) -> Result<String, LaunchError>;
}
pub(super) fn launch_title(cwd: &str) -> String {
    let bytes = cwd.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let windows = drive || cwd.starts_with("\\\\");
    let drive_root = drive && cwd.len() == 3;
    let basename = if drive_root {
        ""
    } else if windows {
        cwd.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
    } else {
        cwd.trim_end_matches('/').rsplit('/').next().unwrap_or("")
    };
    let source = if basename.is_empty() { cwd } else { basename };
    let mut title = String::new();
    let mut units = 0;
    for ch in source.chars() {
        if units + ch.len_utf16() > 200 {
            break;
        }
        title.push(ch);
        units += ch.len_utf16();
    }
    title
}
pub struct ServiceLaunchResolver<'a, D: LaunchDirectoryPolicy> {
    service: &'a mut crate::Service,
    directories: &'a D,
}
impl<'a, D: LaunchDirectoryPolicy> ServiceLaunchResolver<'a, D> {
    pub fn new(service: &'a mut crate::Service, directories: &'a D) -> Self {
        Self {
            service,
            directories,
        }
    }
    fn target(&self, session_id: SessionId) -> Result<LaunchTarget, LaunchError> {
        let snapshot = self
            .service
            .host()
            .snapshot()
            .map_err(|_| LaunchError::Unavailable)?;
        if !snapshot
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            return Err(LaunchError::ReceiptUnavailable);
        }
        // A durable creation receipt proves only the session. In particular it
        // cannot identify the original tab after tabs were closed/reordered.
        Ok(LaunchTarget::Session {
            session: session_id,
            tab: None,
        })
    }
    fn receipt(&self, request: SessionId) -> Result<CreationRequestReceipt, LaunchError> {
        self.service
            .host()
            .creation_request_receipt(request)
            .map_err(|_| LaunchError::Unavailable)
    }
}
impl LaunchAuthority for crate::Service {
    fn check_owned(&self) -> Result<(), LaunchError> {
        self.assert_owned().map_err(|_| LaunchError::StaleOwner)
    }
}
impl<D: LaunchDirectoryPolicy> LaunchAuthority for ServiceLaunchResolver<'_, D> {
    fn check_owned(&self) -> Result<(), LaunchError> {
        self.service.check_owned()
    }
}
impl<D: LaunchDirectoryPolicy> LaunchSessionResolver for ServiceLaunchResolver<'_, D> {
    fn resolve(
        &mut self,
        intent: &DesktopLaunchIntent,
        kind: LaunchRequestKind,
    ) -> Result<LaunchTarget, LaunchError> {
        self.check_owned()?;
        intent.validate()?;
        match &intent.action {
            LaunchAction::Show {} => Ok(LaunchTarget::Show {}),
            LaunchAction::OpenSession { session } => {
                let snapshot = self
                    .service
                    .host()
                    .snapshot()
                    .map_err(|_| LaunchError::Unavailable)?;
                if snapshot.sessions.iter().any(|item| item.id == *session) {
                    Ok(LaunchTarget::Session {
                        session: *session,
                        tab: None,
                    })
                } else {
                    Err(LaunchError::NotFound)
                }
            }
            LaunchAction::NewSession { cwd } => {
                if let CreationRequestReceipt::SessionCreated { session_id } =
                    self.receipt(intent.request_id)?
                {
                    return self.target(session_id);
                }
                if kind == LaunchRequestKind::Known {
                    return Err(LaunchError::ReceiptUnavailable);
                }
                let cwd = self.directories.validate_directory(cwd)?;
                shellfox_core::validation::cwd(&cwd).map_err(|_| LaunchError::Invalid)?;
                let snapshot = self
                    .service
                    .host()
                    .snapshot()
                    .map_err(|_| LaunchError::Unavailable)?;
                self.service
                    .create_application_session(CreateSession {
                        request_id: intent.request_id,
                        title: Some(launch_title(&cwd)),
                        cwd,
                        profile_id: snapshot.preferences.default_profile_id,
                    })
                    .map_err(|_| LaunchError::CreationUncertain)?;
                match self.receipt(intent.request_id)? {
                    CreationRequestReceipt::SessionCreated { session_id } => {
                        self.target(session_id)
                    }
                    CreationRequestReceipt::ReceiptUnavailable => {
                        Err(LaunchError::ReceiptUnavailable)
                    }
                }
            }
        }
    }
}
