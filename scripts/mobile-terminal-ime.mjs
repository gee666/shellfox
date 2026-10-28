import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import path from 'node:path';

async function connectWebView(port) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const target = targets.find(item => item.type === 'page');
  assert(target, 'No test WebView CDP target');
  const url = new URL(target.webSocketDebuggerUrl);
  url.host = `127.0.0.1:${port}`;
  const socket = new WebSocket(url);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let nextId = 0;
  const pending = new Map();
  socket.onmessage = event => {
    const response = JSON.parse(event.data);
    const request = pending.get(response.id);
    if (!request) return;
    pending.delete(response.id);
    if (response.error) request.reject(new Error(JSON.stringify(response.error)));
    else request.resolve(response.result);
  };
  return {
    close: () => socket.close(),
    async evaluate(fn) {
      const response = await new Promise((resolve, reject) => {
        const id = ++nextId;
        const timer = setTimeout(() => { pending.delete(id); reject(new Error('WebView CDP timed out')); }, 15000);
        pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
        socket.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression: `(${fn.toString()})()`, returnByValue: true } }));
      });
      assert(!response.exceptionDetails, JSON.stringify(response.exceptionDetails));
      return response.result.value;
    },
  };
}

/** Actual Gboard taps, not adb input text or synthesized DOM input events. */
export async function checkGboardInput({ adb, shell, tap, waitFor, output }) {
  assert.match(shell('wm', 'size'), /Physical size: 1080x2400/, 'Gboard fixture requires the documented portrait API 35 AVD');
  assert.match(shell('settings', 'get', 'secure', 'default_input_method'), /com\.google\.android\.inputmethod\.latin\//);
  await tap('Keyboard');
  await waitFor(() => /mIsInputViewShown=true/.test(shell('dumpsys', 'input_method')), 'Gboard opens for native typing');
  const pid = shell('pidof', 'com.shellfox.mobile.terminaltest').trim().split(' ')[0];
  const port = adb(['forward', 'tcp:0', `localabstract:webview_devtools_remote_${pid}`]).trim();
  let cdp;
  try {
    cdp = await connectWebView(port);
    await cdp.evaluate(() => {
      const textarea = document.querySelector('.xterm-helper-textarea');
      if (!textarea) throw new Error('No terminal input textarea');
      window.__shellfoxIme = { events: [], inputs: [] };
      const send = window.ReactNativeWebView.postMessage.bind(window.ReactNativeWebView);
      window.ReactNativeWebView.postMessage = raw => {
        const message = JSON.parse(raw);
        if (message.type === 'input') window.__shellfoxIme.inputs.push(message.data);
        send(raw);
      };
      for (const type of ['keydown', 'keyup', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend']) {
        document.addEventListener(type, event => {
          if (event.target !== textarea) return;
          window.__shellfoxIme.events.push({ type: event.type, keyCode: event.keyCode, inputType: event.inputType, data: event.data,
            isComposing: event.isComposing, value: textarea.value, selectionStart: textarea.selectionStart, selectionEnd: textarea.selectionEnd });
        }, true);
      }
    });
    const windows = shell('dumpsys', 'window', 'windows');
    const ime = windows.match(/Window #\d+ Window\{[^}]* InputMethod\}:([\s\S]*?)(?=\n  Window #|$)/)?.[1];
    const top = Number(ime?.match(/touchable region=SkRegion\(\(\d+,(\d+),/)?.[1]);
    assert(top > 0, 'No Gboard window bounds');
    const expected = 'there is one more thing there is one more thing look this is typed twice ';
    const rows = ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
    for (const char of expected) {
      const row = rows.findIndex(keys => keys.includes(char));
      const x = char === ' ' ? 594 : 54 + rows[row].indexOf(char) * 108 + (row === 1 ? 54 : row === 2 ? 162 : 0);
      const y = char === ' ' ? top + 663 : top + 198 + row * 155;
      shell('input', 'tap', String(x), String(y));
    }
    const result = await cdp.evaluate(() => window.__shellfoxIme);
    writeFileSync(path.join(output, 'gboard-events.json'), JSON.stringify({ expected, ...result }, null, 2));
    const nativeText = result.events.filter(event => event.type === 'input').map(event => event.data).join('');
    assert.equal(nativeText, expected, 'The physical Gboard taps must produce the expected native input first');
    assert.equal(result.inputs.join(''), expected, 'Each native edit must reach the terminal exactly once, including intentional repeated phrases');
  } finally {
    cdp?.close();
    adb(['forward', '--remove', `tcp:${port}`]);
    await tap('Keyboard');
  }
}
