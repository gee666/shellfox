use serde_json::Value;
use shellfox_protocol::{manager_v2 as m, *};
#[path = "support/json_presence.rs"]
mod json_presence;
fn check<R: m::Request>(value: &Value) {
    let envelope: m::Envelope<R> = serde_json::from_value(value["request"].clone()).unwrap();
    envelope.validate().unwrap();
    assert_eq!(R::METHOD, value["method"].as_str().unwrap());
    let response: m::BoundReply<R::Response> =
        decode(&serde_json::to_vec(&value["response"]).unwrap()).unwrap();
    if let m::Reply::Success(result) = &response.outcome {
        envelope.payload.validate_response(result).unwrap();
    }
    json_presence::assert_same(&serde_json::to_value(response).unwrap(), &value["response"]);
}
#[test]
fn exact_43_manager_method_golden_payloads_and_responses() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/manager_v2/methods.json")).unwrap();
    let mut names = Vec::new();
    for value in cases {
        let decoded = m::decode_request(&serde_json::to_vec(&value["request"]).unwrap()).unwrap();
        names.push(decoded.command.method_name());
        match decoded.command {
            m::Command::GetSnapshot(_) => check::<m::GetSnapshot>(&value),
            m::Command::GetTerminalProfiles(_) => check::<m::GetTerminalProfiles>(&value),
            m::Command::GetHomeDirectory(_) => check::<m::GetHomeDirectory>(&value),
            m::Command::CreateSession(_) => check::<m::CreateSession>(&value),
            m::Command::AddTab(_) => check::<m::AddTab>(&value),
            m::Command::ActivateSession(_) => check::<m::ActivateSession>(&value),
            m::Command::RenameSession(_) => check::<m::RenameSession>(&value),
            m::Command::RenameTab(_) => check::<m::RenameTab>(&value),
            m::Command::ReorderTabs(_) => check::<m::ReorderTabs>(&value),
            m::Command::SetSessionPinned(_) => check::<m::SetSessionPinned>(&value),
            m::Command::SettleSession(_) => check::<m::SettleSession>(&value),
            m::Command::UnsettleSession(_) => check::<m::UnsettleSession>(&value),
            m::Command::DeleteSession(_) => check::<m::DeleteSession>(&value),
            m::Command::ClearSessionError(_) => check::<m::ClearSessionError>(&value),
            m::Command::SetSessionEnv(_) => check::<m::SetSessionEnv>(&value),
            m::Command::SetTerminalEnv(_) => check::<m::SetTerminalEnv>(&value),
            m::Command::SetVisibleSessions(_) => check::<m::SetVisibleSessions>(&value),
            m::Command::AttachTerminal(_) => check::<m::AttachTerminal>(&value),
            m::Command::TakeTerminalControl(_) => check::<m::TakeTerminalControl>(&value),
            m::Command::WriteTerminal(_) => check::<m::WriteTerminal>(&value),
            m::Command::ResizeTerminal(_) => check::<m::ResizeTerminal>(&value),
            m::Command::AcknowledgeTerminal(_) => check::<m::AcknowledgeTerminal>(&value),
            m::Command::DetachTerminal(_) => check::<m::DetachTerminal>(&value),
            m::Command::CloseTab(_) => check::<m::CloseTab>(&value),
            m::Command::ListFiles(_) => check::<m::ListFiles>(&value),
            m::Command::ReadFile(_) => check::<m::ReadFile>(&value),
            m::Command::WriteFile(_) => check::<m::WriteFile>(&value),
            m::Command::BeginFileRead(_) => check::<m::BeginFileRead>(&value),
            m::Command::ReadFileChunk(_) => check::<m::ReadFileChunk>(&value),
            m::Command::BeginFileWrite(_) => check::<m::BeginFileWrite>(&value),
            m::Command::WriteFileChunk(_) => check::<m::WriteFileChunk>(&value),
            m::Command::FinishFileWrite(_) => check::<m::FinishFileWrite>(&value),
            m::Command::CancelFileTransfer(_) => check::<m::CancelFileTransfer>(&value),
            m::Command::WriteImageFile(_) => check::<m::WriteImageFile>(&value),
            m::Command::ReadFileAsset(_) => check::<m::ReadFileAsset>(&value),
            m::Command::GetSessionStats(_) => check::<m::GetSessionStats>(&value),
            m::Command::ResolveDirectory(_) => check::<m::ResolveDirectory>(&value),
            m::Command::CreateDirectory(_) => check::<m::CreateDirectory>(&value),
            m::Command::CompleteDirectory(_) => check::<m::CompleteDirectory>(&value),
            m::Command::BeginTerminalUpload(_) => check::<m::BeginTerminalUpload>(&value),
            m::Command::WriteTerminalUpload(_) => check::<m::WriteTerminalUpload>(&value),
            m::Command::FinishTerminalUpload(_) => check::<m::FinishTerminalUpload>(&value),
            m::Command::CancelTerminalUpload(_) => check::<m::CancelTerminalUpload>(&value),
        }
    }
    assert_eq!(names, REMOTE_METHODS);
}
