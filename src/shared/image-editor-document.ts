import { FILE_IMAGE_BYTES, FILE_IMAGE_WRITE_BYTES } from './file-schemas';
import { IMAGE_EDITOR_SCRIPT } from './image-editor-script';
import { fileEditorIconSvg, type FileEditorIconName } from './file-editor-icons';
import {
  IMAGE_EDITOR_MAX_DIMENSION, IMAGE_EDITOR_MAX_OPERATIONS, IMAGE_EDITOR_MAX_PIXELS,
  imageEditorMimeSchema, imageEditorOperationsSchema,
  type EditableImage, type ImageEditorOperation,
} from './image-editor-protocol';

interface EditorDocumentOptions {
  image: EditableImage;
  token: string;
  platform: 'desktop' | 'mobile';
  parentOrigin?: string;
  operations?: ImageEditorOperation[];
  fileActions?: boolean;
}
export function imageEditorDocument(options: EditorDocumentOptions): string {
  const mime = imageEditorMimeSchema.parse(options.image.mimeType);
  if (!Number.isInteger(options.image.size) || options.image.size < 0 || options.image.size > FILE_IMAGE_BYTES) throw new Error('Image file is too large.');
  if (!/^[a-zA-Z0-9_-]{16,100}$/.test(options.token)) throw new Error('Invalid editor token.');
  const fileActions = options.platform === 'mobile' && options.fileActions === true;
  const configuration = JSON.stringify({
    token: options.token, platform: options.platform, parentOrigin: options.parentOrigin ?? 'null',
    mime, size: options.image.size, fileActions,
    operations: imageEditorOperationsSchema.parse(options.operations ?? []),
    maxDimension: IMAGE_EDITOR_MAX_DIMENSION, maxPixels: IMAGE_EDITOR_MAX_PIXELS,
    maxOperations: IMAGE_EDITOR_MAX_OPERATIONS, maxInputBytes: FILE_IMAGE_BYTES, maxOutputBytes: FILE_IMAGE_WRITE_BYTES,
  }).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026');
  const csp = `default-src 'none'; script-src 'nonce-${options.token}'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'`;
  const button = (icon: FileEditorIconName, label: string) => `<button id="${icon}" class="icon" type="button" aria-label="${label}" title="${label}">${fileEditorIconSvg(icon)}</button>`;
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}"><meta name="referrer" content="no-referrer">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Image editor</title>
<style>
:root{color-scheme:dark;color:#ddd;background:#17171b;font:12px system-ui}*{box-sizing:border-box}body{margin:0;padding:4px;background:inherit;user-select:none}
.toolbar{display:flex;flex-wrap:nowrap;gap:3px;align-items:center;margin-bottom:4px;overflow-x:auto}
.toolbar.file-actions{gap:2px;justify-content:space-between}.file-actions .icon{width:32px;height:32px}
button{display:inline-flex;align-items:center;justify-content:center;color:inherit;background:#242429;border:1px solid #555;border-radius:3px;padding:4px;cursor:pointer}
button:hover{background:#303037}button:focus-visible{outline:2px solid #548acb;outline-offset:-2px}
button:disabled{opacity:.4;cursor:default}.icon{width:26px;height:26px;flex:none}.icon svg{width:16px;height:16px;flex:none;pointer-events:none}
#save:not(:disabled){color:#b9d8ff;border-color:#548acb}#status{user-select:text}
@media(pointer:coarse){.icon{width:32px;height:32px}}
#viewport{overflow:auto;text-align:center}#stage{display:inline-block;position:relative;vertical-align:top;max-width:100%}
canvas{display:block;max-width:100%;max-height:65vh;width:auto;height:auto;touch-action:none;background:repeating-conic-gradient(#ddd 0% 25%,#fff 0% 50%) 0/16px 16px}
#selection{position:absolute;border:2px dashed #1478df;background:#1478df22;pointer-events:none}
#status{margin:4px 0;min-height:1.3em}
.mobile-file-editor{position:relative;height:100vh;height:100dvh;display:flex;flex-direction:column;overflow:hidden;padding:4px 6px}
.mobile-file-editor .toolbar{margin:0;flex:0 0 32px;overflow:hidden}
.mobile-file-editor #viewport{position:relative;display:flex;align-items:center;justify-content:center;flex:1;min-height:0;overflow:hidden;text-align:initial}
.mobile-file-editor #stage{display:block;flex:none;max-width:none;max-height:none}
.mobile-file-editor canvas{width:100%;height:100%;max-width:none;max-height:none;visibility:hidden}
.mobile-file-editor #status{position:absolute;left:6px;right:6px;bottom:6px;z-index:1;margin:0;padding:4px 6px;border-radius:3px;background:#17171bdd;opacity:0;pointer-events:none}
.mobile-file-editor #status.notice{opacity:1}
</style></head><body${fileActions ? ' class="mobile-file-editor"' : ''}>
<div class="toolbar${fileActions ? ' file-actions' : ''}" role="group" aria-label="Image transformations">
${fileActions ? button('close', 'Close file') + button('more', 'More file actions') : ''}
${button('rotate-left', 'Rotate left')}${button('rotate-right', 'Rotate right')}
${button('flip-horizontal', 'Flip horizontal')}${button('flip-vertical', 'Flip vertical')}
${button('crop', 'Crop')}${button('reset', 'Reset')}${button('save', 'Save image')}</div>
<div id="status"${fileActions ? ' class="notice"' : ''} role="status" aria-live="polite">Loading image…</div>
<div id="viewport"><div id="stage"><canvas id="image" aria-label="Image canvas"></canvas><div id="selection" hidden></div></div></div>
<script id="configuration" type="application/json" nonce="${options.token}">${configuration}</script>
<script nonce="${options.token}">${IMAGE_EDITOR_SCRIPT}</script></body></html>`;
}
