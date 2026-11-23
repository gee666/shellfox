//! Same pure inhibitor algorithm coverage used by the public power integration target.
use super::*;

#[test]
fn linux_acquires_separate_lid_inhibitor_and_reports_denial() {
    let mut calls = Vec::new();
    let requests = acquire_linux_inhibitors(|what| {
        calls.push(what.to_owned());
        if what == "handle-lid-switch" {
            Err(PowerError("policy denied".into()))
        } else {
            Ok(42)
        }
    })
    .unwrap();
    assert_eq!(calls, ["idle:sleep", "handle-lid-switch"]);
    assert_eq!(requests.primary, 42);
    assert!(requests.lid.is_none());
    assert!(requests.partial_reason.unwrap().contains("policy denied"));
    let requests = acquire_linux_inhibitors(|what| Ok::<_, PowerError>(what.to_owned())).unwrap();
    assert_eq!(requests.lid.as_deref(), Some("handle-lid-switch"));
    assert!(requests.partial_reason.is_none());
    let mut calls = 0;
    assert!(acquire_linux_inhibitors::<()>(|_| {
        calls += 1;
        Err(PowerError("denied".into()))
    })
    .is_err());
    assert_eq!(calls, 1);
}
