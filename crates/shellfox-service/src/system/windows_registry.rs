use super::{error, registry::*, SystemResult, SystemScope};
use shellfox_protocol::ErrorCode;
use std::collections::BTreeMap;
mod api;
use api::*;

pub struct NativeSystemRegistry;
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn path(slot: RegistrySlot) -> (Key, String) {
    let user = (-2147483647isize) as Key;
    let machine = (-2147483646isize) as Key;
    match slot {
        RegistrySlot::UserPath => (user, "Environment".into()),
        RegistrySlot::SystemPath => (
            machine,
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment".into(),
        ),
        RegistrySlot::Explorer { scope, background }
        | RegistrySlot::LegacyExplorer { scope, background } => (
            if scope == SystemScope::User {
                user
            } else {
                machine
            },
            format!(
                "Software\\Classes\\Directory\\{}shell\\{}",
                if background { "Background\\" } else { "" },
                if matches!(slot, RegistrySlot::LegacyExplorer { .. }) {
                    "PiManager"
                } else {
                    "Shellfox"
                }
            ),
        ),
    }
}
const FIELDS: &[(RegistryField, &str)] = &[
    (RegistryField::Owner, "ShellfoxOwner"),
    (RegistryField::Label, ""),
    (RegistryField::MenuLabel, "MUIVerb"),
    (RegistryField::Icon, "Icon"),
    (RegistryField::Selection, "MultiSelectModel"),
];
impl SystemRegistry for NativeSystemRegistry {
    fn read(&self, slot: RegistrySlot) -> SystemResult<RegistryState> {
        let (root, name) = path(slot);
        let Some(key) = open(root, &name, 1)? else {
            return Ok(RegistryState::default());
        };
        let (before, subkeys, count) = info(key.0)?;
        let mut values = BTreeMap::new();
        let mut command_revision = 0;
        let mut foreign = false;
        if matches!(slot, RegistrySlot::UserPath | RegistrySlot::SystemPath) {
            if let Some(value) = value(key.0, "Path")? {
                values.insert(RegistryField::Path, value);
            }
        } else {
            for &(field, name) in FIELDS {
                let name = if field == RegistryField::Owner
                    && matches!(slot, RegistrySlot::LegacyExplorer { .. })
                {
                    "PiManagerOwner"
                } else {
                    name
                };
                if let Some(value) = value(key.0, name)? {
                    values.insert(field, value);
                }
            }
            if count as usize > values.len() || subkeys > 1 {
                foreign = true;
            }
            if let Some(command) = open(key.0, "command", 1)? {
                let (revision, subkeys, count) = info(command.0)?;
                command_revision = revision;
                if let Some(value) = value(command.0, "")? {
                    values.insert(RegistryField::Command, value);
                }
                if info(command.0)?.0 != revision {
                    return Err(conflict());
                }
                if subkeys > 0 || count > u32::from(values.contains_key(&RegistryField::Command)) {
                    foreign = true;
                }
            } else if subkeys > 0 {
                foreign = true;
            }
        }
        if info(key.0)?.0 != before {
            return Err(conflict());
        }
        Ok(RegistryState {
            exists: true,
            revision: RegistryRevision {
                key: before,
                command: command_revision,
            },
            values,
            foreign_content: foreign,
        })
    }
    fn compare_replace(
        &self,
        slot: RegistrySlot,
        expected: &RegistryState,
        desired: Option<&RegistryState>,
    ) -> SystemResult<()> {
        if matches!(
            slot,
            RegistrySlot::SystemPath
                | RegistrySlot::Explorer {
                    scope: SystemScope::System,
                    ..
                }
                | RegistrySlot::LegacyExplorer {
                    scope: SystemScope::System,
                    ..
                }
        ) {
            return Err(error(ErrorCode::Unsupported,"System installation requires a separately authorized elevation/package policy; no elevation was attempted."));
        }
        if desired.is_none() && matches!(slot, RegistrySlot::UserPath | RegistrySlot::SystemPath) {
            return Err(error(
                ErrorCode::Validation,
                "Environment keys cannot be deleted by CLI integration.",
            ));
        }
        if matches!(slot, RegistrySlot::UserPath | RegistrySlot::SystemPath) {
            if desired.is_some_and(|state| {
                state
                    .values
                    .keys()
                    .any(|field| *field != RegistryField::Path)
            }) {
                return Err(error(
                    ErrorCode::Validation,
                    "CLI installation may change only PATH.",
                ));
            }
        } else {
            let owner = if matches!(slot, RegistrySlot::LegacyExplorer { .. }) {
                "PiManager/native-v1"
            } else {
                "Shellfox/native-v1"
            };
            if expected.exists && expected.text(RegistryField::Owner) != Some(owner) {
                return Err(error(
                    ErrorCode::AuthFailed,
                    "Foreign registry integration was not changed.",
                ));
            }
            if matches!(slot, RegistrySlot::LegacyExplorer { .. }) && desired.is_some() {
                return Err(error(
                    ErrorCode::Validation,
                    "Legacy keys are removal-only.",
                ));
            }
            if desired.is_some_and(|state| {
                state.text(RegistryField::Owner) != Some(owner)
                    || state.values.contains_key(&RegistryField::Path)
            }) {
                return Err(error(
                    ErrorCode::Validation,
                    "Invalid owned Explorer registry values.",
                ));
            }
        }
        if &self.read(slot)? != expected {
            return Err(conflict());
        }
        if expected.foreign_content {
            return Err(error(
                ErrorCode::AuthFailed,
                "Registry integration contains unrelated values and was not changed.",
            ));
        }
        let (root, name) = path(slot);
        let Some(desired) = desired else {
            if expected.exists {
                let name = wide(&name);
                // SAFETY: fixed exact owned integration key, revision checked above.
                if unsafe { RegDeleteTreeW(root, name.as_ptr()) } != 0 {
                    return Err(unavailable(
                        "Owned registry integration could not be removed.",
                    ));
                }
            }
            return Ok(());
        };
        let key = if let Some(key) = open(root, &name, 1 | 2 | 4)? {
            if &self.read(slot)? != expected {
                return Err(conflict());
            }
            key
        } else {
            create(root, &name, !expected.exists)?
        };
        for (&field, value) in &desired.values {
            let value_name = if field == RegistryField::Path {
                "Path"
            } else if field == RegistryField::Command {
                ""
            } else {
                FIELDS
                    .iter()
                    .find(|(known, _)| *known == field)
                    .map(|(_, name)| *name)
                    .ok_or_else(|| unavailable("Unsupported registry field."))?
            };
            if field == RegistryField::Command {
                let command = create(key.0, "command", false)?;
                set(command.0, value_name, value)?;
            } else {
                set(key.0, value_name, value)?;
            }
        }
        for field in expected
            .values
            .keys()
            .filter(|field| !desired.values.contains_key(field))
        {
            let name = if *field == RegistryField::Path {
                "Path"
            } else {
                FIELDS
                    .iter()
                    .find(|(known, _)| known == field)
                    .map(|(_, name)| *name)
                    .ok_or_else(|| unavailable("Unsupported registry deletion."))?
            };
            let name = wide(name);
            // SAFETY: a fixed owned value, not arbitrary request text.
            let result = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
            if result != 0 && result != 2 {
                return Err(unavailable("Registry value removal failed."));
            }
        }
        Ok(())
    }
}
