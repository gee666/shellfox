//! Changes made in Electron after the first import (a downgrade to 0.9 and
//! back), merged from a private snapshot. Sessions match by id. Deletions in
//! Electron are not carried over: a deleted session coming back loses nothing.
use crate::{activity_store::activity_unix_ms, storage, *};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

/// SHA-256 of each top-level Electron settings field, as of the last sync.
/// Digests, not values: the environment can hold secrets.
pub type ElectronSettingsDigest = BTreeMap<String, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElectronMerge {
    pub added: usize,
    pub updated: usize,
    /// Settings fields taken from Electron.
    pub settings: Vec<String>,
    /// Electron's current settings, the baseline for the next merge.
    pub digest: ElectronSettingsDigest,
}

impl Host {
    /// Merges what Electron changed after `since_ms`:
    /// - a session this store lacks is added if Electron created or used it since;
    /// - a known session takes Electron's title, pin, settlement, environment
    ///   and time when Electron changed it since and later than this store did;
    /// - each settings field whose Electron value differs from `baseline`
    ///   replaces this store's value. Without a baseline, settings stay.
    pub fn merge_electron(
        &self,
        snapshot: &Path,
        since_ms: u64,
        baseline: Option<&ElectronSettingsDigest>,
    ) -> Result<ElectronMerge> {
        let (path, _checked_artifacts) = crate::storage_guard::legacy(snapshot)?;
        let mut imported =
            crate::legacy::read_legacy(&path, SettingsImportPolicy::IncludeEnvironment)?;
        let incoming = to_object(&imported.settings)?;
        let mut merge = ElectronMerge {
            digest: incoming
                .iter()
                .map(|(key, value)| (key.clone(), digest(value)))
                .collect(),
            ..ElectronMerge::default()
        };
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage::storage_error)?;
        let mut data = storage::load(&tx)?;
        let since = |at: &str| activity_unix_ms(at).is_ok_and(|ms| ms > since_ms);
        for session in imported.sessions {
            match data.sessions.iter_mut().find(|s| s.id == session.id) {
                Some(current) => {
                    let newer = since(&session.updated_at)
                        && activity_unix_ms(&session.updated_at)?
                            > activity_unix_ms(&current.updated_at).unwrap_or(0);
                    if newer {
                        // The cwd stays: 0.9 never changes it, Rust may have.
                        current.title = session.title;
                        current.pinned_at = session.pinned_at;
                        current.settled_at = session.settled_at;
                        current.env = session.env;
                        current.updated_at = session.updated_at;
                        merge.updated += 1;
                    }
                }
                None if since(&session.created_at) || since(&session.updated_at) => {
                    if let Some(facts) = imported.creation_facts.remove(&session.id) {
                        data.creation_facts.insert(session.id, facts);
                    }
                    data.sessions.push(session);
                    merge.added += 1;
                }
                None => {}
            }
        }
        if let Some(baseline) = baseline {
            let mut current = to_object(&crate::settings_store::read(&tx)?)?;
            for (key, value) in incoming {
                if baseline.get(&key) != merge.digest.get(&key) && current.get(&key) != Some(&value)
                {
                    current.insert(key.clone(), value);
                    merge.settings.push(key);
                }
            }
            if !merge.settings.is_empty() {
                let settings: Settings = serde_json::from_value(current.into())
                    .map_err(|_| Error::new(ErrorCode::Validation, "Invalid merged settings."))?;
                crate::settings_store::write(&tx, &settings)?;
                crate::settings_store::project(&settings, &mut data)?;
            }
        }
        if merge.added + merge.updated == 0 && merge.settings.is_empty() {
            return Ok(merge);
        }
        data.repair_selection();
        let revision = data
            .state
            .revision
            .checked_add(1)
            .ok_or_else(|| storage::storage_error("revision overflow"))?;
        data.state.revision = revision;
        data.state.last_change = Some(ChangedEvent {
            revision,
            reason: if merge.settings.is_empty() {
                ChangeReason::Sessions
            } else {
                ChangeReason::Settings
            },
            select_session_id: data.state.selection.session_id,
        });
        storage::save(&tx, &data)?;
        if let Some(runtime) = runtime {
            runtime.check_commit()?;
        }
        self.commit(tx, revision)?;
        Ok(merge)
    }
}

fn to_object(settings: &Settings) -> Result<serde_json::Map<String, serde_json::Value>> {
    match serde_json::to_value(settings) {
        Ok(serde_json::Value::Object(object)) => Ok(object),
        _ => Err(Error::new(
            ErrorCode::Internal,
            "Settings are not an object.",
        )),
    }
}

/// Over sorted keys, whatever the map order, so saved baselines stay valid.
fn digest(value: &serde_json::Value) -> String {
    Sha256::digest(sorted(value).to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn sorted(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.into_iter()
                .map(|key| (key.clone(), sorted(&map[key])))
                .collect::<serde_json::Map<_, _>>()
                .into()
        }
        serde_json::Value::Array(items) => items.iter().map(sorted).collect(),
        other => other.clone(),
    }
}

#[cfg(test)]
mod digest_tests {
    #[test]
    fn a_digest_ignores_key_order_and_matches_sorted_json() {
        let a: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":[{"y":2,"x":1}]}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"a":[{"x":1,"y":2}],"b":1}"#).unwrap();
        assert_eq!(super::digest(&a), super::digest(&b));
        // The baseline earlier builds saved hashed sorted compact JSON.
        let saved = r#"{"a":[{"x":1,"y":2}],"b":1}"#;
        use sha2::Digest;
        let expected: String = sha2::Sha256::digest(saved.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(super::digest(&a), expected);
    }
}
