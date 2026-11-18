use shellfox_ghostty_vt::{Error, Terminal, HARD_PRESENTATION_CELLS, MAX_PRESENTATION_CELLS};

#[test]
fn explicit_hard_budget_supports_a_full_million_cell_frame(
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(MAX_PRESENTATION_CELLS, 262_144);
    assert_eq!(HARD_PRESENTATION_CELLS, 1_048_576);
    let mut terminal = Terminal::new(1024, 1024)?;
    let mut view = terminal.new_presentation_view()?;
    assert!(matches!(terminal.view_render(&mut view), Err(Error(-6))));
    let frame = terminal.view_render_with_limit(&mut view, HARD_PRESENTATION_CELLS)?;
    assert_eq!(frame.screen.cells.len(), HARD_PRESENTATION_CELLS);
    assert_eq!(frame.row_metadata.len(), 1024);
    assert!(terminal.drain_replies()?.is_empty());
    assert!(Terminal::new(1025, 1024).is_err());
    Ok(())
}
