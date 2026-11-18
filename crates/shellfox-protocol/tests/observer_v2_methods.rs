use serde_json::Value;
#[path = "support/json_presence.rs"]
mod json_presence;
use shellfox_protocol::{observer_v2::*, *};
fn check<R: observer_v2::Request>(value: &Value) {
    let envelope: observer_v2::Envelope<R> =
        serde_json::from_value(value["request"].clone()).unwrap();
    envelope.validate().unwrap();
    assert_eq!(R::METHOD, value["method"].as_str().unwrap());
    assert_eq!(R::ALLOWED, value["allowed"].as_bool().unwrap());
    let response: BoundReply<R::Response> =
        decode(&serde_json::to_vec(&value["response"]).unwrap()).unwrap();
    let encoded = serde_json::to_value(response).unwrap();
    json_presence::assert_same(&encoded, &value["response"]);
}
#[test]
fn contract_manifest_matches_routes_limits_and_profile_policy() {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/observer_v2/contract.json")).unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/observer_v2/methods.json")).unwrap();
    assert_eq!(
        manifest["methods"],
        serde_json::to_value(REMOTE_METHODS).unwrap()
    );
    let denied: Vec<_> = cases
        .iter()
        .filter(|v| v["allowed"] == false)
        .map(|v| v["method"].clone())
        .collect();
    assert_eq!(
        manifest["profileDeniedMethods"],
        serde_json::to_value(denied).unwrap()
    );
    for (name, endpoint) in [
        ("discover", Endpoint::DiscoverV2),
        ("pair", Endpoint::PairV2),
        ("request", Endpoint::RequestV2),
        ("events", Endpoint::EventsV2),
        ("disconnect", Endpoint::DisconnectV2),
    ] {
        assert_eq!(manifest["routes"][name], endpoint.path());
    }
    assert_eq!(manifest["limits"]["requestBytes"], MAX_REQUEST_BYTES);
    assert_eq!(manifest["limits"]["responseBytes"], MAX_RESPONSE_BYTES);
    assert_eq!(
        manifest["limits"]["recoveryDecodedBytes"],
        MAX_RECOVERY_BYTES
    );
    assert_eq!(manifest["limits"]["dataDecodedBytes"], MAX_DATA_BYTES);
    assert_eq!(manifest["limits"]["pollDecodedBytes"], MAX_POLL_DATA_BYTES);
    assert_eq!(manifest["limits"]["goSharedOperations"], 12);
    assert_eq!(manifest["limits"]["goSharedPolls"], 64);
}
#[test]
fn all_43_golden_mappings_validate_typed_payloads_responses_and_profile_roles() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/observer_v2/methods.json")).unwrap();
    assert_eq!(cases.len(), 43);
    let mut names = Vec::new();
    for value in cases {
        let request = decode_request(&serde_json::to_vec(&value["request"]).unwrap()).unwrap();
        names.push(request.command.method_name());
        assert_eq!(
            request.command.is_profile_allowed(),
            value["allowed"].as_bool().unwrap()
        );
        match request.command {
            observer_v2::Command::GetSnapshot(_) => check::<observer_v2::GetSnapshot>(&value),
            observer_v2::Command::GetTerminalProfiles(_) => {
                check::<observer_v2::GetTerminalProfiles>(&value)
            }
            observer_v2::Command::GetHomeDirectory(_) => {
                check::<observer_v2::GetHomeDirectory>(&value)
            }
            observer_v2::Command::CreateSession(_) => check::<observer_v2::CreateSession>(&value),
            observer_v2::Command::AddTab(_) => check::<observer_v2::AddTab>(&value),
            observer_v2::Command::ActivateSession(_) => {
                check::<observer_v2::ActivateSession>(&value)
            }
            observer_v2::Command::RenameSession(_) => check::<observer_v2::RenameSession>(&value),
            observer_v2::Command::RenameTab(_) => check::<observer_v2::RenameTab>(&value),
            observer_v2::Command::ReorderTabs(_) => check::<observer_v2::ReorderTabs>(&value),
            observer_v2::Command::SetSessionPinned(_) => {
                check::<observer_v2::SetSessionPinned>(&value)
            }
            observer_v2::Command::SettleSession(_) => check::<observer_v2::SettleSession>(&value),
            observer_v2::Command::UnsettleSession(_) => {
                check::<observer_v2::UnsettleSession>(&value)
            }
            observer_v2::Command::DeleteSession(_) => check::<observer_v2::DeleteSession>(&value),
            observer_v2::Command::ClearSessionError(_) => {
                check::<observer_v2::ClearSessionError>(&value)
            }
            observer_v2::Command::SetSessionEnv(_) => check::<observer_v2::SetSessionEnv>(&value),
            observer_v2::Command::SetTerminalEnv(_) => check::<observer_v2::SetTerminalEnv>(&value),
            observer_v2::Command::SetVisibleSessions(_) => {
                check::<observer_v2::SetVisibleSessions>(&value)
            }
            observer_v2::Command::AttachTerminal(_) => check::<observer_v2::AttachTerminal>(&value),
            observer_v2::Command::TakeTerminalControl(_) => {
                check::<observer_v2::TakeTerminalControl>(&value)
            }
            observer_v2::Command::WriteTerminal(_) => check::<observer_v2::WriteTerminal>(&value),
            observer_v2::Command::ResizeTerminal(_) => check::<observer_v2::ResizeTerminal>(&value),
            observer_v2::Command::AcknowledgeTerminal(_) => {
                check::<observer_v2::AcknowledgeTerminal>(&value)
            }
            observer_v2::Command::DetachTerminal(_) => check::<observer_v2::DetachTerminal>(&value),
            observer_v2::Command::CloseTab(_) => check::<observer_v2::CloseTab>(&value),
            observer_v2::Command::ListFiles(_) => check::<observer_v2::ListFiles>(&value),
            observer_v2::Command::ReadFile(_) => check::<observer_v2::ReadFile>(&value),
            observer_v2::Command::WriteFile(_) => check::<observer_v2::WriteFile>(&value),
            observer_v2::Command::BeginFileRead(_) => check::<observer_v2::BeginFileRead>(&value),
            observer_v2::Command::ReadFileChunk(_) => check::<observer_v2::ReadFileChunk>(&value),
            observer_v2::Command::BeginFileWrite(_) => check::<observer_v2::BeginFileWrite>(&value),
            observer_v2::Command::WriteFileChunk(_) => check::<observer_v2::WriteFileChunk>(&value),
            observer_v2::Command::FinishFileWrite(_) => {
                check::<observer_v2::FinishFileWrite>(&value)
            }
            observer_v2::Command::CancelFileTransfer(_) => {
                check::<observer_v2::CancelFileTransfer>(&value)
            }
            observer_v2::Command::WriteImageFile(_) => check::<observer_v2::WriteImageFile>(&value),
            observer_v2::Command::ReadFileAsset(_) => check::<observer_v2::ReadFileAsset>(&value),
            observer_v2::Command::GetSessionStats(_) => {
                check::<observer_v2::GetSessionStats>(&value)
            }
            observer_v2::Command::ResolveDirectory(_) => {
                check::<observer_v2::ResolveDirectory>(&value)
            }
            observer_v2::Command::CreateDirectory(_) => {
                check::<observer_v2::CreateDirectory>(&value)
            }
            observer_v2::Command::CompleteDirectory(_) => {
                check::<observer_v2::CompleteDirectory>(&value)
            }
            observer_v2::Command::BeginTerminalUpload(_) => {
                check::<observer_v2::BeginTerminalUpload>(&value)
            }
            observer_v2::Command::WriteTerminalUpload(_) => {
                check::<observer_v2::WriteTerminalUpload>(&value)
            }
            observer_v2::Command::FinishTerminalUpload(_) => {
                check::<observer_v2::FinishTerminalUpload>(&value)
            }
            observer_v2::Command::CancelTerminalUpload(_) => {
                check::<observer_v2::CancelTerminalUpload>(&value)
            }
        }
    }
    assert_eq!(names, REMOTE_METHODS);
}
