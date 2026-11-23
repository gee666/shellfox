use super::api;
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    ssh,
};
pub(super) enum Expected {
    Profiles,
    Imported,
    SelectedKey,
}
pub(super) struct Bound {
    pub action: NativeAction,
    pub expected: Expected,
}
pub(super) fn bind(request: api::Request) -> anyhow::Result<Bound> {
    let (action, expected) = match request {
        api::Request::SshList => (NativeAction::Ssh(ssh::Request::List), Expected::Profiles),
        api::Request::SshSave(input) => {
            let password = match input.password {
                api::Password::Preserve => ssh::Password::Keep,
                api::Password::Clear => ssh::Password::Clear,
                api::Password::Replace(value) => ssh::Password::set(value)?,
            };
            let input = ssh::Input {
                id: input
                    .id
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid SSH profile ID."))?,
                name: input.name,
                host: input.host,
                port: input.port,
                user: input.user,
                password,
                key_file: input.key_file,
                remote_cwd: input.remote_cwd,
            };
            (
                NativeAction::Ssh(ssh::Request::Save(input)),
                Expected::Profiles,
            )
        }
        api::Request::SshDelete(id) => (
            NativeAction::Ssh(ssh::Request::Delete {
                id: id
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid SSH profile ID."))?,
            }),
            Expected::Profiles,
        ),
        api::Request::SshImport => (NativeAction::Ssh(ssh::Request::Import), Expected::Imported),
        api::Request::ValidateSshKey(selected) => (
            NativeAction::Ssh(ssh::Request::ValidateKey { selected }),
            Expected::SelectedKey,
        ),
        _ => anyhow::bail!("This request is not a typed native SSH capability."),
    };
    action.validate()?;
    Ok(Bound { action, expected })
}
pub(super) fn project(
    expected: Expected,
    response: NativeResponse,
) -> anyhow::Result<api::Response> {
    match (expected, response) {
        (Expected::Profiles, NativeResponse::Ssh(ssh::Response::Profiles(profiles))) => {
            Ok(api::Response::SshProfiles(
                profiles
                    .into_iter()
                    .map(|profile| api::SshProfile {
                        id: profile.id.to_string(),
                        name: profile.name,
                        host: profile.host,
                        port: profile.port,
                        user: profile.user,
                        has_password: profile.has_password,
                        key_file: profile.key_file,
                        remote_cwd: profile.remote_cwd,
                    })
                    .collect(),
            ))
        }
        (Expected::Imported, NativeResponse::Ssh(ssh::Response::Imported(imported))) => {
            Ok(api::Response::SshImported {
                profiles: imported
                    .profiles
                    .into_iter()
                    .map(|profile| api::SshProfile {
                        id: profile.id.to_string(),
                        name: profile.name,
                        host: profile.host,
                        port: profile.port,
                        user: profile.user,
                        has_password: profile.has_password,
                        key_file: profile.key_file,
                        remote_cwd: profile.remote_cwd,
                    })
                    .collect(),
                found: imported.found,
                added: imported.added,
                updated: imported.updated,
            })
        }
        (Expected::SelectedKey, NativeResponse::Ssh(ssh::Response::SelectedKey(info))) => {
            ssh::SelectedKey::new(std::path::PathBuf::from(&info.path))?;
            Ok(api::Response::KeyFile(Some(info.path)))
        }
        _ => anyhow::bail!(
            "Invalid typed SSH response from the shared host; no success was fabricated."
        ),
    }
}
