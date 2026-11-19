use super::*;

/// An inclusive terminal cell in the retained buffer, not a text insertion boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPoint {
    pub x: u16,
    pub row: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PresentationBuffer {
    Primary,
    Alternate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PresentationSelectionKind {
    Range,
    Word,
    Line,
    All,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationSelection {
    /// Original inclusive start cell for Range; normalized first cell otherwise.
    pub anchor: HistoryPoint,
    /// Original inclusive end cell for Range; normalized last cell otherwise.
    pub focus: HistoryPoint,
    pub rectangle: bool,
    pub buffer: PresentationBuffer,
    pub kind: PresentationSelectionKind,
    pub view_top_row: u64,
    pub total_rows: u64,
}
#[repr(C)]
#[derive(Default)]
struct NativeSelection {
    anchor_row: u64,
    focus_row: u64,
    view_top_row: u64,
    total_rows: u64,
    anchor_x: u16,
    focus_x: u16,
    cols: u16,
    rectangle: u8,
    alternate: u8,
    kind: u8,
}
unsafe extern "C" {
    fn sf_presentation_selection(
        vt: *mut c_void,
        view: *mut c_void,
        out: *mut NativeSelection,
    ) -> i32;
}
impl Engine {
    pub fn view_selection(
        &mut self,
        view: &mut PresentationView,
    ) -> Result<Option<PresentationSelection>> {
        self.check_view(view)?;
        let mut value = NativeSelection::default();
        // SAFETY: owner identity is checked before touching refs. The synchronous
        // public tracked-point queries never return raw refs or borrowed memory.
        let code =
            unsafe { sf_presentation_selection(self.ptr.as_ptr(), view.ptr.as_ptr(), &mut value) };
        if code == -4 {
            return Ok(None);
        }
        check(code)?;
        if value.cols != self.cols
            || value.rectangle > 1
            || value.alternate > 1
            || value.kind > 3
            || value.anchor_x >= value.cols
            || value.focus_x >= value.cols
            || value.anchor_row >= value.total_rows
            || value.focus_row >= value.total_rows
            || value.total_rows < u64::from(self.rows)
            || value.view_top_row > value.total_rows - u64::from(self.rows)
            || (value.kind != 0
                && (value.rectangle != 0
                    || (value.anchor_row, value.anchor_x) > (value.focus_row, value.focus_x)))
        {
            return Err(-2);
        }
        Ok(Some(PresentationSelection {
            anchor: HistoryPoint {
                x: value.anchor_x,
                row: value.anchor_row,
            },
            focus: HistoryPoint {
                x: value.focus_x,
                row: value.focus_row,
            },
            rectangle: value.rectangle != 0,
            buffer: if value.alternate == 0 {
                PresentationBuffer::Primary
            } else {
                PresentationBuffer::Alternate
            },
            kind: match value.kind {
                0 => PresentationSelectionKind::Range,
                1 => PresentationSelectionKind::Word,
                2 => PresentationSelectionKind::Line,
                3 => PresentationSelectionKind::All,
                _ => return Err(-2),
            },
            view_top_row: value.view_top_row,
            total_rows: value.total_rows,
        }))
    }
}
