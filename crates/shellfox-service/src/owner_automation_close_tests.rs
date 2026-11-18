use super::*;
#[test]
#[ignore="explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn joined_raw_close_proof_survives_metadata_failure_and_retry_without_relaunch() {
    let(scratch,mut owned,helper)=service();let service=&mut owned.0;
    let spec=input(&helper,scratch.path(),vec!["describe".into()]);
    let run=service.start_automation(spec.clone(),9).unwrap();finish(service,run.id);
    let shown=show(service,run.id);let tab:TabId=shown.tab_id.unwrap().to_string().parse().unwrap();
    let identity=service.terminals[&tab].runtime.identity();
    let prepared=service.host.prepare_close_tab(tab).unwrap();
    service.close_external_for_tab(tab).unwrap();
    assert!(!service.terminals.contains_key(&tab));
    assert!(matches!(service.automation.closed_external.get(&tab),Some(ClosedExternal::Acknowledged(proof)) if proof.runtime_identity()==identity&&proof.raw_cleanup_confirmed()));
    assert!(service.automation.runs.retired().is_empty());
    // Actual metadata revision change invalidates the prepared close, as output
    // activity or another committed metadata action can do during cleanup.
    service.host.rename_session(identity.session_id,"concurrent metadata fixture").unwrap();
    assert!(service.finish_close(prepared,&[tab]).is_err());
    assert!(service.host.snapshot().unwrap().sessions.iter().any(|session|session.id==identity.session_id));
    assert!(service.automation.closed_external.contains_key(&tab));
    assert!(service.start_automation(spec,9).is_err());
    service.close_tab(tab).unwrap();
    assert!(!service.automation.closed_external.contains_key(&tab));
    assert!(service.terminals.is_empty());service.shutdown().unwrap();
}
