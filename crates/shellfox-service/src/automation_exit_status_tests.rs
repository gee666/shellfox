use super::*;
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn high_bit_natural_status_matches_actual_node_unsigned_dword_oracle() {
    // Actual Node spawn and CLI process.exitCode forwarding both returned these
    // exact DWORDs from the owned helper, signal=null, no stdout/stderr.
    let mut f = Fixture::new();
    for code in [7u32, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
        let text = code.to_string();
        let input = f.input(&["exit-code", &text], false);
        let run = f.start(input, 1);
        let (stdout, stderr, actual) = f.collect(run.id, 1);
        assert!(stdout.is_empty() && stderr.is_empty());
        assert_eq!(actual, i64::from(code));
        f.manager.begin_stop(AutomationStop { id: run.id }).unwrap();
        let dto = &f.manager.runs[&run.id].dto;
        assert_eq!(dto.exit_code, Some(i64::from(code)));
        assert!(dto.signal.is_none());
        assert_eq!(f.manager.runs[&run.id].outcome, Some(code as i32));
    }
}
