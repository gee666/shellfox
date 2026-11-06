// Only this application-owned script runs in the editor. Image bytes never become markup or code.
export const IMAGE_EDITOR_SCRIPT = String.raw`
(function () {
  'use strict';
  const config = JSON.parse(document.getElementById('configuration').textContent);
  const canvas = document.getElementById('image');
  const viewport = document.getElementById('viewport');
  const stage = document.getElementById('stage');
  const selection = document.getElementById('selection');
  const status = document.getElementById('status');
  const saveButton = document.getElementById('save');
  const cropButton = document.getElementById('crop');
  let original = null, operations = [], ready = false, loading = false, locked = true;
  let anchor = null, selectedArea = null, requestingSave = false;
  function send(message) {
    const payload = Object.assign({ token: config.token }, message);
    if (config.platform === 'mobile') window.ReactNativeWebView.postMessage(JSON.stringify(payload));
    else parent.postMessage(payload, config.parentOrigin === 'null' ? '*' : config.parentOrigin);
  }
  function error(message) { status.textContent = message; status.classList.add('notice'); }
  function report(message) { error(message); send({ type: 'error', message: message.slice(0, 500) }); }
  function controls() {
    document.querySelectorAll('button').forEach(button => { button.disabled = !ready || locked; });
    saveButton.disabled = !ready || locked || operations.length === 0;
    saveButton.setAttribute('aria-busy', String(ready && locked));
    saveButton.title = ready && locked ? 'Saving…' : 'Save image';
    cropButton.disabled = !ready || locked || selectedArea === null;
    document.getElementById('reset').disabled = !ready || locked || (!operations.length && !selectedArea);
    // Navigation remains available if image decoding fails, never during a save.
    if (config.fileActions) ['close', 'more'].forEach(action => { document.getElementById(action).disabled = ready && locked; });
  }
  function lock(value) { locked = value; controls(); }
  function fitViewport() {
    if (!config.fileActions || !ready) return;
    const bounds = viewport.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    const scale = Math.min(bounds.width / canvas.width, bounds.height / canvas.height);
    stage.style.width = canvas.width * scale + 'px';
    stage.style.height = canvas.height * scale + 'px';
    canvas.style.visibility = 'visible';
  }
  function state() {
    controls(); fitViewport(); status.classList.remove('notice');
    status.textContent = canvas.width + ' × ' + canvas.height + (operations.length ? ' · Unsaved' : '');
    send({ type: 'state', dirty: operations.length > 0, width: canvas.width, height: canvas.height, operations });
  }
  function clearSelection() { anchor = null; selectedArea = null; selection.hidden = true; controls(); }
  function bounded(width, height) {
    return Number.isInteger(width) && Number.isInteger(height) && width > 0 && height > 0
      && width <= config.maxDimension && height <= config.maxDimension && width * height <= config.maxPixels;
  }
  // Reject decompression bombs before asking the browser to decode the image.
  function dimensions(dataUrl) {
    const bytes = atob(dataUrl.slice(dataUrl.indexOf(',') + 1));
    if (bytes.length > config.maxInputBytes) throw new Error('Image file is too large.');
    const byte = i => bytes.charCodeAt(i);
    const be16 = i => byte(i) * 256 + byte(i + 1);
    const be32 = i => be16(i) * 65536 + be16(i + 2);
    const le16 = i => byte(i) + byte(i + 1) * 256;
    const le24 = i => le16(i) + byte(i + 2) * 65536;
    if (config.mime === 'image/png' && bytes.slice(0, 8) === '\x89PNG\r\n\x1a\n'
      && bytes.slice(12, 16) === 'IHDR') {
      let offset = 8;
      while (offset + 12 <= bytes.length) {
        const type = bytes.slice(offset + 4, offset + 8);
        if (type === 'acTL') throw new Error('Animated PNG editing is not supported.');
        if (type === 'IDAT' || type === 'IEND') break;
        const length = be32(offset);
        if (length > bytes.length - offset - 12) break;
        offset += length + 12;
      }
      return [be32(16), be32(20)];
    }
    if (config.mime === 'image/jpeg' && be16(0) === 65496) {
      let offset = 2;
      while (offset + 3 < bytes.length) {
        if (byte(offset++) !== 255) break;
        while (byte(offset) === 255) offset++;
        const marker = byte(offset++);
        if (marker === 217 || marker === 218) break;
        if (marker === 1 || (marker >= 208 && marker <= 215)) continue;
        const length = be16(offset);
        if (length < 2 || offset + length > bytes.length) break;
        if ([192,193,194,195,197,198,199,201,202,203,205,206,207].includes(marker)) {
          return [be16(offset + 5), be16(offset + 3)];
        }
        offset += length;
      }
    }
    if (config.mime === 'image/webp' && bytes.slice(0, 4) === 'RIFF' && bytes.slice(8, 12) === 'WEBP') {
      const type = bytes.slice(12, 16);
      if (type === 'VP8X') {
        if ((byte(20) & 2) !== 0) throw new Error('Animated WebP editing is not supported.');
        return [le24(24) + 1, le24(27) + 1];
      }
      if (type === 'VP8L' && byte(20) === 47) return [1 + ((byte(21) + byte(22) * 256) & 16383),
        1 + (((byte(22) >> 6) + byte(23) * 4 + byte(24) * 1024) & 16383)];
      if (type === 'VP8 ' && bytes.slice(23, 26) === '\x9d\x01\x2a') return [le16(26) & 16383, le16(28) & 16383];
    }
    throw new Error('Unsupported or damaged image header.');
  }
  function copy(source, width, height) {
    canvas.width = width; canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Canvas is unavailable.');
    context.drawImage(source, 0, 0);
  }
  function apply(operation) {
    const width = canvas.width, height = canvas.height;
    const temp = document.createElement('canvas');
    const rotated = operation.type === 'rotate';
    temp.width = rotated ? height : operation.type === 'crop' ? operation.width : width;
    temp.height = rotated ? width : operation.type === 'crop' ? operation.height : height;
    if (!bounded(temp.width, temp.height)) throw new Error('Invalid image dimensions.');
    const context = temp.getContext('2d');
    if (!context) throw new Error('Canvas is unavailable.');
    if (rotated) {
      context.translate(operation.direction === 'right' ? height : 0, operation.direction === 'left' ? width : 0);
      context.rotate(operation.direction === 'right' ? Math.PI / 2 : -Math.PI / 2);
      context.drawImage(canvas, 0, 0);
    } else if (operation.type === 'flip') {
      const horizontal = operation.axis === 'horizontal';
      context.translate(horizontal ? width : 0, horizontal ? 0 : height);
      context.scale(horizontal ? -1 : 1, horizontal ? 1 : -1);
      context.drawImage(canvas, 0, 0);
    } else {
      if (operation.x < 0 || operation.y < 0 || operation.x + operation.width > width
        || operation.y + operation.height > height) throw new Error('Crop must stay inside the image.');
      context.drawImage(canvas, operation.x, operation.y, operation.width, operation.height, 0, 0, temp.width, temp.height);
    }
    copy(temp, temp.width, temp.height);
    temp.width = 0; temp.height = 0;
  }
  function commit(operation) {
    if (!ready || locked) return;
    if (operations.length >= config.maxOperations) { error('Save or reset before making more edits.'); return; }
    try { apply(operation); operations.push(operation); clearSelection(); state(); }
    catch (cause) { error(cause instanceof Error ? cause.message : 'Image edit failed.'); }
  }
  ['left', 'right'].forEach(direction => document.getElementById('rotate-' + direction).onclick = () => commit({ type: 'rotate', direction }));
  ['horizontal', 'vertical'].forEach(axis => document.getElementById('flip-' + axis).onclick = () => commit({ type: 'flip', axis }));
  document.getElementById('reset').onclick = () => {
    if (!ready || locked) return;
    copy(original, original.naturalWidth || original.width, original.naturalHeight || original.height);
    operations = []; clearSelection(); state();
  };
  cropButton.onclick = () => { if (selectedArea) commit(Object.assign({ type: 'crop' }, selectedArea)); };
  function requestSave() {
    if (!ready || locked || !operations.length) return;
    requestingSave = true; lock(true); send({ type: 'save-request' });
  }
  saveButton.onclick = requestSave;
  if (config.fileActions) ['close', 'more'].forEach(action => {
    document.getElementById(action).onclick = () => {
      if (!(ready && locked)) send({ type: 'file-action', action });
    };
  });
  document.addEventListener('keydown', event => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') { event.preventDefault(); requestSave(); }
    if (event.key === 'Escape' && selectedArea && !locked) { event.preventDefault(); clearSelection(); }
  });
  function point(event) {
    const rect = canvas.getBoundingClientRect();
    return { x: Math.max(0, Math.min(canvas.width, Math.round((event.clientX - rect.left) * canvas.width / rect.width))),
      y: Math.max(0, Math.min(canvas.height, Math.round((event.clientY - rect.top) * canvas.height / rect.height))) };
  }
  function drawSelection() {
    selection.hidden = selectedArea === null;
    if (selectedArea) {
      selection.style.left = selectedArea.x / canvas.width * 100 + '%';
      selection.style.top = selectedArea.y / canvas.height * 100 + '%';
      selection.style.width = selectedArea.width / canvas.width * 100 + '%';
      selection.style.height = selectedArea.height / canvas.height * 100 + '%';
    }
    controls();
  }
  canvas.onpointerdown = event => {
    if (!ready || locked || !event.isPrimary || event.button !== 0) return;
    anchor = Object.assign({ id: event.pointerId }, point(event)); selectedArea = null; drawSelection();
    canvas.setPointerCapture(event.pointerId); event.preventDefault();
  };
  canvas.onpointermove = event => {
    if (!anchor || event.pointerId !== anchor.id || locked) return;
    const end = point(event), width = Math.abs(end.x - anchor.x), height = Math.abs(end.y - anchor.y);
    selectedArea = width && height ? { x: Math.min(anchor.x, end.x), y: Math.min(anchor.y, end.y), width, height } : null;
    drawSelection();
  };
  canvas.onpointerup = event => {
    if (anchor && event.pointerId !== anchor.id) return;
    if (anchor) canvas.onpointermove(event);
    anchor = null;
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
  };
  canvas.onpointercancel = event => { if (anchor && event.pointerId === anchor.id) clearSelection(); };
  function exportImage() {
    if (!ready || (locked && !requestingSave)) return;
    requestingSave = false; lock(true);
    try {
      let output = canvas;
      if (config.mime === 'image/jpeg') {
        output = document.createElement('canvas'); output.width = canvas.width; output.height = canvas.height;
        const context = output.getContext('2d');
        if (!context) throw new Error('Canvas is unavailable.');
        context.fillStyle = '#fff'; context.fillRect(0, 0, output.width, output.height); context.drawImage(canvas, 0, 0);
      }
      const dataUrl = output.toDataURL(config.mime, 0.92);
      if (output !== canvas) { output.width = 0; output.height = 0; }
      if (!dataUrl.startsWith('data:' + config.mime + ';base64,')) throw new Error('This device cannot encode the original image format.');
      const base64 = dataUrl.slice(dataUrl.indexOf(',') + 1);
      const bytes = base64.length / 4 * 3 - (base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0);
      if (bytes > config.maxOutputBytes) throw new Error('Edited image exceeds the ' + Math.floor(config.maxOutputBytes / 1024) + ' KiB save limit. Crop the image before saving.');
      send({ type: 'export', dataUrl });
    } catch (cause) { lock(false); report(cause instanceof Error ? cause.message : 'Image encoding failed.'); }
  }
  function receive(event) {
    if (config.platform === 'desktop' && (event.source !== parent || event.origin !== config.parentOrigin)) return;
    let message = event.data;
    if (typeof message === 'string') {
      if (message.length > Math.ceil(config.maxInputBytes / 3) * 4 + 512) return;
      try { message = JSON.parse(message); } catch (_) { return; }
    }
    if (!message || typeof message !== 'object' || message.token !== config.token) return;
    if (message.type === 'load') {
      if (Object.keys(message).length !== 3 || loading || ready || typeof message.dataUrl !== 'string'
        || message.dataUrl.length > Math.ceil(config.maxInputBytes / 3) * 4 + 128
        || !message.dataUrl.startsWith('data:' + config.mime + ';base64,')) return;
      loadImage(message.dataUrl); return;
    }
    if (Object.keys(message).length !== 2) return;
    if (message.type === 'export') exportImage();
    else if (message.type === 'unlock' && ready) { requestingSave = false; lock(false); }
    else if (message.type === 'saved' && ready) {
      const baseline = document.createElement('canvas'); baseline.width = canvas.width; baseline.height = canvas.height;
      const context = baseline.getContext('2d');
      if (!context) { report('Canvas is unavailable.'); return; }
      context.drawImage(canvas, 0, 0); original = baseline; operations = []; lock(false); clearSelection(); state();
    }
  }
  function loadImage(dataUrl) {
    loading = true;
    try {
    const size = dimensions(dataUrl);
    if (!bounded(size[0], size[1])) throw new Error('Image exceeds the 8192-pixel edge or 16-megapixel limit.');
    const image = new Image();
    image.onload = () => {
      try {
        if (!bounded(image.naturalWidth, image.naturalHeight)) throw new Error('Decoded image is too large.');
        original = image; copy(image, image.naturalWidth, image.naturalHeight);
        config.operations.forEach(operation => { apply(operation); operations.push(operation); });
        ready = true; lock(false); state();
      } catch (cause) { report(cause instanceof Error ? cause.message : 'Image loading failed.'); }
    };
    image.onerror = () => report('Could not decode image.');
    image.src = dataUrl;
    } catch (cause) { report(cause instanceof Error ? cause.message : 'Image loading failed.'); }
  }
  window.addEventListener('message', receive);
  if (config.platform === 'mobile') document.addEventListener('message', receive);
  if (config.fileActions) {
    window.addEventListener('resize', fitViewport);
    if (typeof ResizeObserver !== 'undefined') new ResizeObserver(fitViewport).observe(viewport);
  }
  lock(true);
  send({ type: 'request-image' });
})();
`;
