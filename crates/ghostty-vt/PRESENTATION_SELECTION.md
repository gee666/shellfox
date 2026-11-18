# Presentation selection endpoint contract

`Terminal::view_selection` returns checked public tracked-reference coordinates, not coordinates inferred from selected-cell masks. The query does not install or change the controller selection, scroll its viewport, feed data, drain replies, or sort an existing range.

## Coordinate and boundary units

Both `anchor` and `focus` are **inclusive terminal-cell endpoints**, exactly like `Selection::Range`. They are not insertion boundaries, character offsets, UTF-8 byte offsets or UTF-16 offsets.

- `HistoryPoint.x` is a cell column.
- `HistoryPoint.row` is an absolute row within the currently retained buffer, starting at zero at its oldest retained row. It is not an output sequence or a permanent history identifier. Pruning can change row numbers.
- `view_top_row` is that view's current top row in the same coordinate space. An endpoint above or below the visible viewport remains absolute; it is not clipped or replaced with a visible cell.
- `buffer` identifies primary or alternate. Each buffer retains its own view selection. This query reports the currently active buffer, including its context explicitly.
- `total_rows` bounds that retained buffer at query time.

`Range` preserves the stored anchor and focus roles, including reversed drags and both rectangular diagonals. `Word`, `Line` and `All` are programmatic selections. They use Ghostty's public forward-normalization API, and their `kind` makes that normalized meaning explicit. They do not claim an original pointer-drag direction.

A range with equal endpoint cells still selects that cell's text. It is not a collapsed caret. No selection is represented by `None`; caret presentation is separate. If tracked endpoints become coincident after a terminal mutation, the query does not invent missing gesture history or a reverse-direction flag.

## Conversion to AccessKit text boundaries

AccessKit 0.25.2 defines `TextPosition.character_index` as an index into the published text run's `character_lengths`, or its length for an end position. Each entry describes a selectable character, which need not be a single Unicode scalar. The lengths themselves are UTF-8 byte lengths. Therefore neither a cell column nor a byte/scalar/UTF-16 count can be copied into `character_index` directly.

Use the actual sampled text-run mapping from terminal cells to published selectable characters, preserving actual VT cell clusters:

1. Validate the frame's source identity, buffer context and sampling/version boundary. Consume only Eirik's compiled typed carrier. Do not combine endpoints from one owner transaction with cells from another.
2. For a linear selection, compare the endpoint cell coordinates in buffer reading order. Keep the original anchor/focus roles.
3. For a forward range, map the anchor to the boundary **before** its complete published character, and the focus to the boundary **after** its complete published character.
4. For a reverse range, map the anchor to the boundary **after** its complete published character, and the focus to the boundary **before** its complete published character.
5. An equal-cell range selects one complete selectable character using before/after boundaries. Equal cell coordinates alone do not supply a reverse gesture direction.

A wide-character tail belongs to the same selectable character as its width-two leading cell. A combining sequence or ZWJ emoji stored together in one rendered cell must not be split merely because it contains several scalars. Conversely, do not re-cluster text across separate VT cells just because concatenating them forms a Unicode grapheme. Follow the actual published `Cell.text`/run mapping, not `column + 1` or `UTF-16 offset + 1`.

With Ghostty grapheme-clustering mode 2027 enabled, the fixture row `A界é👩‍💻Z` has cell text that can be published as entries `A`, `界`, `é`, `👩‍💻`, `Z`. The wide `界` occupies terminal columns 1 and 2, while `é` occupies column 3. Thus:

- Inclusive cells anchor 1, focus 2 select `界` with AccessKit boundaries anchor 1, focus 2.
- Reversed inclusive cells anchor 2, focus 1 select the same `界`, but use AccessKit boundaries anchor 2, focus 1.
- Equal inclusive cells anchor 3, focus 3 select the whole `é` with boundaries anchor 2, focus 3, not a collapsed boundary and not a split combining sequence.

These indices assume that exact published run. They are not a universal formula for every text-run representation. With mode 2027 disabled, the pinned library stores this example's `👩` plus ZWJ in one wide cell and `💻` in another wide cell. Selecting the first pair of columns copies only `👩` plus ZWJ. The tests explicitly cover both modes; the endpoint query does not fabricate a joined emoji or alter terminal modes to make a conversion work.

A hard line terminator has no terminal cell endpoint. If the text model publishes an LF or CRLF, AccessKit requires it to be one `character_lengths` entry. Crossing a hard line naturally crosses that entry; do not blindly advance a row-end endpoint past a newline entry that it did not name. Soft-wrapped rows must follow the actual published logical-line mapping.

## Limits that remain explicit

- An offscreen endpoint is representable in AT only when its corresponding text run is actually published. A visible-only tree cannot fabricate a node or clamp the endpoint to a viewport edge. It must report the selection as unavailable or partial under an explicit policy.
- A multiline rectangular selection is disjoint text. One ordinary AccessKit `TextSelection` cannot faithfully encode it as a single contiguous range. Do not silently flatten it. The query retains the rectangle and original corners for a future suitable AT representation.
- Width-zero cells can include non-text spacers. A known wide leading-cell span maps its tail to that grapheme. An unmapped spacer/head or omitted blank cell must not be assigned a guessed character boundary. The query reports cells, not a complete cell-to-AT mapping; another checked public query would be needed if the renderer's published mapping cannot resolve the case.
- If either tracked endpoint is discarded, the query returns `None`. It does not clamp the survivor or manufacture a smaller range. The endpoint query itself does not clear the view's stored refs.
- Endpoint correctness does not establish full AT selection parity. Offscreen history publication, rectangle/disjoint support, run-unit mapping and stale-frame rejection still belong to the consumer.

The native tests cover reverse drags with identical selected masks, rectangle diagonals, offscreen coordinates, pruning, primary/alternate isolation, reflow and Unicode cells. No internal Ghostty ABI or fork is used.
