//! Closed private-store versions. Only existing validated approvals migrate.
use crate::{error, Approval, ViewPermission};
use serde::{Deserialize, Serialize};
use shellfox_protocol::{AppError, ErrorCode};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u8,
    approvals: serde::de::IgnoredAny,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Legacy {
    version: u8,
    approvals: Vec<Approval>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
enum FixedPolicy {
    #[serde(rename = "control-full-account-v1")]
    ControlFullAccount,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    peer_id: String,
    hostname: String,
    approved_at_millis: u64,
    policy: FixedPolicy,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Current {
    version: u8,
    approvals: Vec<Record>,
}
pub(crate) fn normalized(mut approval: Approval) -> Result<Approval, AppError> {
    approval.validate()?;
    // This applies the user's fixed approved-device policy, not a roots-empty
    // interpretation. Grant creation separately mints ApprovedFullAccount.
    approval.permission = ViewPermission::Write;
    approval.file_roots.clear();
    Ok(approval)
}
pub(crate) fn validate(records: &[Approval]) -> Result<(), AppError> {
    if records.len() > 1000 {
        return Err(invalid());
    }
    let mut seen = std::collections::HashSet::new();
    for record in records {
        record.validate()?;
        if !seen.insert(&record.peer_id) {
            return Err(invalid());
        }
    }
    Ok(())
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Approval>, AppError> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let _ = header.approvals;
    let records = match header.version {
        1 => {
            let legacy: Legacy = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if legacy.version != 1 {
                return Err(invalid());
            }
            legacy.approvals
        }
        2 => {
            let current: Current = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if current.version != 2 {
                return Err(invalid());
            }
            current
                .approvals
                .into_iter()
                .map(|record| Approval {
                    peer_id: record.peer_id,
                    hostname: record.hostname,
                    approved_at_millis: record.approved_at_millis,
                    permission: ViewPermission::Write,
                    file_roots: Vec::new(),
                })
                .collect()
        }
        _ => return Err(invalid()),
    };
    validate(&records)?;
    records.into_iter().map(normalized).collect()
}
// Fixture encoder only while the native approval write protocol is gated.
#[cfg(test)]
pub(crate) fn encode(records: &[Approval]) -> Result<Vec<u8>, AppError> {
    validate(records)?;
    let approvals = records
        .iter()
        .map(|record| Record {
            peer_id: record.peer_id.clone(),
            hostname: record.hostname.clone(),
            approved_at_millis: record.approved_at_millis,
            policy: FixedPolicy::ControlFullAccount,
        })
        .collect();
    serde_json::to_vec(&Current {
        version: 2,
        approvals,
    })
    .map_err(|_| invalid())
}
fn invalid() -> AppError {
    error(
        ErrorCode::StorageFailed,
        "Private approval policy record is invalid or unsupported.",
        false,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn legacy() -> serde_json::Value {
        json!({"version":1,"approvals":[{"peerId":"n-approved","hostname":"owned","approvedAtMillis":1,"permission":"read-only","fileRoots":[]}]})
    }
    #[test]
    fn malformed_legacy_or_future_current_policy_is_not_partially_approved() {
        let mut bad = legacy();
        bad["approvals"][0]["fileRoots"] = json!(["relative-root"]);
        assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
        let current = json!({"version":2,"approvals":[{"peerId":"n-approved","hostname":"owned","approvedAtMillis":1,"policy":"future-admin-all"}]});
        assert!(decode(&serde_json::to_vec(&current).unwrap()).is_err());
        let mut bad = legacy();
        let mut invalid = bad["approvals"][0].clone();
        invalid["peerId"] = json!("invalid peer id");
        bad["approvals"].as_array_mut().unwrap().push(invalid);
        assert!(
            decode(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "a malformed mixed file cannot install even its valid prefix"
        );
        let mut bad = legacy();
        bad["pending"] = json!([{"peerId":"n-future"}]);
        assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[test]
    fn only_closed_known_approved_records_migrate_to_explicit_full_account_policy() {
        let decoded = decode(&serde_json::to_vec(&legacy()).unwrap()).unwrap();
        assert_eq!(decoded[0].permission, ViewPermission::Write);
        let encoded = encode(&decoded).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["approvals"][0]["policy"], "control-full-account-v1");
        assert_eq!(decode(&encoded).unwrap()[0].peer_id, "n-approved");
        for version in [0, 3, 255] {
            let mut bad = legacy();
            bad["version"] = json!(version);
            assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut bad = legacy();
        bad["approvals"][0]["pending"] = json!(true);
        assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = legacy();
        bad["approvals"][0]["permission"] = json!("future-admin");
        assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
        let empty = decode(br#"{"version":1,"approvals":[]}"#).unwrap();
        assert!(
            empty.is_empty(),
            "pending/revoked peers are never manufactured"
        );
    }
}
