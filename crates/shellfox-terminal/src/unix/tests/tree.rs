use crate::unix::tree::{Identity, Kernel, Record, Signal, Tree};
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[derive(Default)]
struct Model {
    rows: BTreeMap<u32, Record>,
    exited: BTreeSet<Identity>,
    signals: Vec<(Identity, Signal)>,
    fail_pin: bool,
    fail_kill: Option<u32>,
    fail_resume: Option<u32>,
}
fn id(pid: u32) -> Identity {
    Identity {
        pid,
        ticks: u64::from(pid),
    }
}
fn row(pid: u32, parent: u32, session: u32) -> Record {
    Record {
        identity: id(pid),
        parent,
        session,
        state: b'S',
    }
}
impl Kernel for Model {
    type Handle = Identity;
    fn snapshot(&mut self) -> Result<Vec<Record>> {
        Ok(self.rows.values().cloned().collect())
    }
    fn inspect(&mut self, identity: &Identity) -> Result<Option<Record>> {
        let row = self.rows.get(&identity.pid).cloned();
        if let Some(row) = &row {
            ensure!(row.identity == *identity, "stale identity");
        }
        Ok(row)
    }
    fn pin(&mut self, record: &Record) -> Result<Option<Identity>> {
        ensure!(!self.fail_pin, "injected pin failure");
        Ok(Some(record.identity.clone()))
    }
    fn exited(&mut self, identity: &Identity) -> Result<bool> {
        Ok(self.exited.contains(identity))
    }
    fn signal(&mut self, identity: &Identity, signal: Signal) -> Result<()> {
        self.signals.push((identity.clone(), signal));
        ensure!(
            signal != Signal::Kill || self.fail_kill != Some(identity.pid),
            "kill failure"
        );
        ensure!(
            signal != Signal::Continue || self.fail_resume != Some(identity.pid),
            "resume failure"
        );
        match signal {
            Signal::Kill => {
                self.exited.insert(identity.clone());
                self.rows.remove(&identity.pid);
            }
            Signal::Stop => {
                self.rows.get_mut(&identity.pid).unwrap().state = b'T';
            }
            Signal::Continue => {
                self.rows.get_mut(&identity.pid).unwrap().state = b'S';
            }
        }
        Ok(())
    }
    fn pause(&mut self) {}
}
fn fixture() -> (Tree<Identity>, Model) {
    let rows = [
        row(10, 1, 10),
        row(20, 10, 10),
        row(30, 20, 30),
        row(99, 1, 99),
    ];
    (
        Tree::new(id(10), id(10)),
        Model {
            rows: rows.into_iter().map(|r| (r.identity.pid, r)).collect(),
            ..Model::default()
        },
    )
}
const BUDGET: Duration = Duration::from_millis(50);
#[test]
fn closes_detached_descendants_before_root_without_touching_unrelated() {
    let (mut tree, mut model) = fixture();
    assert!(!tree.root_exited(&mut model).unwrap());
    tree.close(&mut model, BUDGET).unwrap();
    assert!(tree.root_exited(&mut model).unwrap());
    assert!(model.rows.contains_key(&99));
    assert_eq!(
        model
            .signals
            .iter()
            .filter(|(_, s)| *s == Signal::Kill)
            .map(|(id, _)| id.pid)
            .collect::<Vec<_>>(),
        [20, 30, 10]
    );
}
#[test]
fn root_exit_does_not_release_previously_owned_descendants() {
    let (mut tree, mut model) = fixture();
    tree.track(&mut model).unwrap();
    model.rows.remove(&10);
    model.exited.insert(id(10));
    model.rows.get_mut(&20).unwrap().parent = 1;
    tree.close(&mut model, BUDGET).unwrap();
    assert!(model.exited.contains(&id(20)) && model.exited.contains(&id(30)));
    assert!(model.rows.contains_key(&99));
}
#[test]
fn stale_sid_anchor_never_authorizes_a_new_session() {
    let (mut tree, mut model) = fixture();
    tree.track(&mut model).unwrap();
    model.exited.insert(id(10));
    let replacement = model.rows.get_mut(&10).unwrap();
    replacement.identity.ticks = 1000;
    assert!(tree
        .close(&mut model, BUDGET)
        .unwrap_err()
        .to_string()
        .contains("anchor was lost"));
    assert!(!model.signals.iter().any(|(_, s)| *s == Signal::Kill));
    assert!(model.rows.values().all(|r| r.state != b'T'));
}
#[test]
fn failed_acquisition_restores_every_process_stopped_by_this_attempt() {
    let (mut tree, mut model) = fixture();
    model.fail_pin = true;
    assert!(tree.close(&mut model, BUDGET).is_err());
    assert_eq!(model.rows[&10].state, b'S');
    assert!(!model.signals.iter().any(|(_, s)| *s == Signal::Kill));
    model.fail_pin = false;
    tree.close(&mut model, BUDGET).unwrap();
}
#[test]
fn partial_commit_and_failed_rollback_keep_exact_handles_for_retry() {
    let (mut tree, mut model) = fixture();
    model.fail_kill = Some(30);
    model.fail_resume = Some(10);
    assert!(tree.close(&mut model, BUDGET).is_err());
    assert!(model.exited.contains(&id(20)));
    // One failed restore must not suppress restoration of the other survivor.
    assert_eq!(model.rows[&30].state, b'S');
    assert_eq!(model.rows[&10].state, b'T');
    model.fail_kill = None;
    model.fail_resume = None;
    tree.close(&mut model, BUDGET).unwrap();
    assert!(model.rows.contains_key(&99));
}
#[test]
fn completed_commands_do_not_exhaust_the_live_identity_bound() {
    let (mut tree, mut model) = fixture();
    for pid in 200..500 {
        model.rows.insert(pid, row(pid, 10, 10));
        tree.track(&mut model).unwrap();
        model.exited.insert(id(pid));
        model.rows.remove(&pid);
    }
    tree.close(&mut model, BUDGET).unwrap();
    assert!(model.rows.contains_key(&99));
}
#[cfg(target_os = "linux")]
#[test]
fn persistent_supervisor_cleans_descendants_without_stopping_itself() {
    let pid = std::process::id();
    let root = id(pid);
    let child = pid + 10;
    let mut model = Model {
        rows: [row(pid, 1, pid), row(child, pid, pid)]
            .into_iter()
            .map(|r| (r.identity.pid, r))
            .collect(),
        ..Model::default()
    };
    let mut tree = Tree::supervisor_self(root.clone(), root.clone()).unwrap();
    tree.close(&mut model, BUDGET).unwrap();
    assert!(model.rows.contains_key(&pid));
    assert!(model.exited.contains(&id(child)));
    assert!(!model.signals.iter().any(|(identity, _)| *identity == root));
}
#[test]
fn preexisting_stop_is_not_resumed_by_failed_cleanup() {
    let (mut tree, mut model) = fixture();
    model.rows.get_mut(&10).unwrap().state = b'T';
    model.fail_pin = true;
    assert!(tree.close(&mut model, BUDGET).is_err());
    assert_eq!(model.rows[&10].state, b'T');
    assert!(!model.signals.contains(&(id(10), Signal::Continue)));
}
