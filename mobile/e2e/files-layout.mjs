import assert from 'node:assert/strict';
import { controlBounds } from '../../scripts/mobile-e2e-controls.mjs';

export function byId(nodes, id) {
  return nodes.find(node => (node.resource === id || node.resource.endsWith(`/${id}`)) && controlBounds(node));
}
export function byLabel(nodes, label) {
  return nodes.find(node => node.label === label && controlBounds(node)) ?? nodes.find(node => node.text === label && controlBounds(node));
}
export function safeViewport(metrics) {
  assert(metrics && metrics.scale > 0 && metrics.width > 0 && metrics.height > 0, 'Missing QA native viewport metrics.');
  const { scale, insets } = metrics;
  return { left: insets.left * scale, top: insets.top * scale, right: (metrics.width - insets.right) * scale, bottom: (metrics.height - insets.bottom) * scale };
}
const centerX = box => (box.left + box.right) / 2;
const centerY = box => (box.top + box.bottom) / 2;
export function assertFilesLayout(nodes, metrics, { controls, last, directory = false }) {
  const viewport = safeViewport(metrics), toolbar = controlBounds(byId(nodes, 'files-toolbar')), content = controlBounds(byId(nodes, 'files-content'));
  assert(toolbar && content, 'Permanent toolbar and content must expose their native layout bounds.');
  assert((toolbar.bottom - toolbar.top) / metrics.scale <= 38, 'Permanent navigation must fit one 36dp row.');
  const boxes = controls.map(label => { const box = controlBounds(byLabel(nodes, label)); assert(box, `Missing visible icon: ${label}`); return box; });
  assert(Math.max(...boxes.map(centerY)) - Math.min(...boxes.map(centerY)) <= 2 * metrics.scale, 'Toolbar icons must share one vertical center.');
  for (const box of boxes) assert(box.left >= viewport.left - 1 && box.right <= viewport.right + 1, 'Toolbar controls must not overflow horizontally.');
  const lastBox = controlBounds(byLabel(nodes, last));
  assert(lastBox && viewport.right - lastBox.right <= 9 * metrics.scale, 'The final action must align to the right, not leave a dead action area.');
  const contentRatio = (content.bottom - content.top) / (viewport.bottom - viewport.top);
  assert(contentRatio >= .8, `File content uses only ${(contentRatio * 100).toFixed(1)}% of the safe viewport.`);
  assert(content.right - content.left >= (viewport.right - viewport.left) * .95, 'Content must use the viewport width.');
  for (const id of ['files-upload', 'files-download']) {
    const button = controlBounds(byId(nodes, id)), glyph = controlBounds(byId(nodes, `${id}-glyph`));
    if (button) {
      assert(glyph, `${id} needs measurable centered native glyph bounds.`);
      assert(Math.abs(centerX(button) - centerX(glyph)) <= metrics.scale && Math.abs(centerY(button) - centerY(glyph)) <= metrics.scale, `${id} arrow is off center.`);
    }
  }
  if (directory) {
    const rows = nodes.filter(node => /^Open (?:file|folder) /.test(node.label) && controlBounds(node));
    assert(rows.length, 'Expected visible folder/file rows.');
    for (const row of rows) {
      const box = controlBounds(row);
      assert(box.right - box.left >= (viewport.right - viewport.left) * .95, `Row ${row.label} has a narrow or indented tap target.`);
      const height = (box.bottom - box.top) / metrics.scale;
      assert(height >= 34 && height <= 42, `Row ${row.label} is not compact: ${height.toFixed(1)}dp.`);
    }
    assert(!nodes.some(node => /^\d+ entries(?: · truncated)?$/.test(node.text)), 'Do not reserve a row for entry-count metadata.');
  }
  return { toolbar, content, viewport, contentRatio };
}
export function assertPathSheetLayout(nodes, metrics, keyboard) {
  const field = controlBounds(nodes.find(node => node.type === 'android.widget.EditText' && node.label === 'File path' && controlBounds(node))), go = controlBounds(byId(nodes, 'files-path-go') ?? byLabel(nodes, 'Go'));
  assert(field && go, 'Path field and its Go icon must be visible.');
  assert(Math.abs(centerY(field) - centerY(go)) <= 2 * metrics.scale, 'Path field and Go icon must be vertically aligned.');
  assert(Math.abs((field.bottom - field.top) - (go.bottom - go.top)) <= 4 * metrics.scale, 'Path field and Go icon must have matching heights.');
  assert(!nodes.some(node => node.text === 'Go'), 'The path action should be an icon, not a text Go button.');
  if (keyboard) {
    const close = controlBounds(byId(nodes, 'files-path-close'));
    assert(close, 'The app path close icon must be visible with the software keyboard.');
    assert([field, go, close].every(box => box.bottom <= keyboard.top + metrics.scale), 'App path input, X and Go must be above the real bottom keyboard, not hidden behind an extract window.');
  }
  return { field, go };
}
export const imageActions = ['Close file', 'More file actions', 'Rotate left', 'Rotate right', 'Flip horizontal', 'Flip vertical', 'Reset', 'Crop', 'Save image'];
export function assertImageToolbar(nodes, metrics) {
  const viewport = safeViewport(metrics);
  assert(!byId(nodes, 'files-toolbar'), 'Image editor must not stack a native toolbar above its child toolbar.');
  const content = controlBounds(byId(nodes, 'files-content'));
  assert(content && (content.bottom - content.top) / (viewport.bottom - viewport.top) >= .8, 'Image editor must retain at least 80% of the safe viewport.');
  const boxes = imageActions.map(label => { const box = controlBounds(byLabel(nodes, label)); assert(box, `Missing image action: ${label}`); return box; });
  assert(Math.max(...boxes.map(centerY)) - Math.min(...boxes.map(centerY)) <= 2 * metrics.scale, 'All nine image actions must share one row.');
  assert(boxes.every(box => box.left >= viewport.left && box.right <= viewport.right), 'Image actions must not overflow horizontally.');
  assert(boxes[0].left < boxes[1].left && boxes[1].left < boxes[2].left, 'Image toolbar must start with Close and More.');
  assert(boxes.at(-1).right >= Math.max(...boxes.map(box => box.right)), 'Save image must be last.');
  assert(viewport.right - boxes.at(-1).right <= 9 * metrics.scale, 'Save image must sit at the far right.');
  return boxes;
}
