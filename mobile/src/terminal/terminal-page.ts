/// <reference lib="dom" />
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { PageKeyboard } from './page-keyboard';
import { hostMessageSchema, type PageMessage } from './protocol';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, derivePalette } from '../../../src/renderer/theme';

type NativeWindow = Window & { ReactNativeWebView?: { postMessage: (data: string) => void } };
const viewId = `page-${Date.now()}-${Math.random().toString(36).slice(2)}`;
let epoch = '', terminal: Terminal | null = null, fit: FitAddon | null = null;
let historical = false, writing = false, selecting = false;
let keyboard: PageKeyboard | null = null;
let fitted = '', fitFrame = 0, fitPending = false;
let theme = derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND).terminal;
const host = document.getElementById('terminal');
if (!host) throw new Error('Missing terminal container.');
const container = host;
const nativeWindow: NativeWindow = window;
const send = (message: PageMessage) => nativeWindow.ReactNativeWebView?.postMessage(JSON.stringify(message));
const identity = () => ({ viewId, epoch });
const emitSize = () => {
  if (!terminal || historical) return;
  const cols = terminal.cols, rows = terminal.rows;
  if (cols < 2 || cols > 500 || rows < 2 || rows > 500) return;
  const key = `${epoch}:${cols}:${rows}`;
  if (key !== fitted) { fitted = key; send({ type: 'resize', ...identity(), cols, rows }); }
};
const fitTerminal = () => {
  fitPending = true;
  cancelAnimationFrame(fitFrame);
  fitFrame = requestAnimationFrame(() => {
    if (!terminal || !fit || writing) return;
    const size = fit.proposeDimensions();
    if (!size) return;
    fitPending = false;
    terminal.resize(Math.max(2, Math.min(500, size.cols)), Math.max(2, Math.min(500, size.rows)));
    emitSize();
  });
};
function createTerminal(cols: number, rows: number, fontSize: number) {
  const focused = !!terminal?.textarea && document.activeElement === terminal.textarea;
  keyboard?.dispose(); terminal?.dispose(); container.replaceChildren();
  // Hold the replay grid until the first output command has painted.
  fitted = ''; historical = true; writing = true; selecting = false; fitPending = false;
  const term = new Terminal({ cols, rows, fontSize, fontFamily: 'Menlo, Consolas, monospace',
    scrollback: 3000, convertEol: false, cursorBlink: true, allowProposedApi: false,
    theme, macOptionIsMeta: true,
    linkHandler: { activate: () => undefined, allowNonHttpProtocols: false },
  });
  terminal = term; fit = new FitAddon(); term.loadAddon(fit); term.open(container);
  const textarea = term.textarea;
  if (textarea) {
    textarea.setAttribute('autocapitalize', 'off'); textarea.setAttribute('autocorrect', 'off');
    textarea.setAttribute('autocomplete', 'off'); textarea.setAttribute('inputmode', 'text');
    textarea.spellcheck = false;
  }
  if (focused) term.focus();
  const keyboardEpoch = epoch;
  keyboard = new PageKeyboard(term, data => {
    if (terminal === term && epoch === keyboardEpoch && !historical && data && !data.includes('\0')) {
      send({ type: 'input', ...identity(), data });
    }
  });
  term.onSelectionChange(() => send({ type: 'selection', ...identity(), text: term.getSelection().slice(0, 65536) }));
}
const seen = new WeakSet<MessageEvent>();
function receive(event: MessageEvent) {
  if (seen.has(event)) return;
  seen.add(event);
  if (typeof event.data !== 'string' || event.data.length > 1600000) return;
  try {
    const parsed = hostMessageSchema.safeParse(JSON.parse(event.data));
    if (!parsed.success || parsed.data.viewId !== viewId) return;
    const message = parsed.data;
    if (message.type === 'clear') {
      epoch = message.epoch; clearLongPress(); anchor = null; start = null;
      keyboard?.dispose(); terminal?.blur(); terminal?.dispose(); terminal = null; fit = null; keyboard = null;
      historical = true; writing = true; selecting = false; fitPending = false;
      cancelAnimationFrame(fitFrame); container.replaceChildren(); return;
    }
    if (message.type === 'reset') {
      epoch = message.epoch; createTerminal(message.cols, message.rows, message.fontSize); return;
    }
    if (message.epoch !== epoch || !terminal) return;
    switch (message.type) {
      case 'output': {
        const term = terminal, renderingEpoch = epoch;
        historical = message.historical; writing = true;
        term.write(message.data, () => {
          // xterm's write callback follows parsing. Wait for its scheduled paint before granting credit.
          requestAnimationFrame(() => requestAnimationFrame(() => {
            if (terminal !== term || epoch !== renderingEpoch) return;
            historical = false; writing = false;
            send({ type: 'rendered', ...identity(), ticket: message.ticket });
            if (message.fit || fitPending) fitTerminal();
          }));
        }); break;
      }
      case 'font': terminal.options.fontSize = message.fontSize; fitTerminal(); break;
      case 'theme': {
        theme = derivePalette(message.accentColor, message.backgroundColor).terminal;
        terminal.options.theme = theme;
        document.documentElement.style.backgroundColor = message.backgroundColor;
        document.body.style.backgroundColor = message.backgroundColor;
        container.style.backgroundColor = message.backgroundColor;
        break;
      }
      case 'paste': keyboard?.paste(message.data); break;
      case 'key': keyboard?.key(message.event); break;
      case 'modifiers': keyboard?.setModifiers(message.modifiers); break;
      case 'focus':
        if (message.focused) {
          terminal.focus();
          if (document.activeElement === terminal.textarea) send({ type: 'focused', ...identity() });
        } else terminal.blur();
        break;
      case 'select': selecting = message.enabled; if (!selecting) terminal.clearSelection(); break;
    }
  } catch {
    send({ type: 'error', ...identity(), message: 'The terminal display could not process a bridge message.' });
  }
}
// Android dispatches on document; iOS dispatches on window. Deduplicate bubbling events.
window.addEventListener('message', receive);
document.addEventListener('message', event => { if (event instanceof MessageEvent) receive(event); });
new ResizeObserver(fitTerminal).observe(container);
window.visualViewport?.addEventListener('resize', fitTerminal);

let anchor: { col: number; row: number } | null = null;
let start: { x: number; y: number } | null = null;
let scrollY = 0, scrollRemainder = 0;
let longPress: ReturnType<typeof setTimeout> | null = null;
let moved = false;
const clearLongPress = () => { if (longPress) clearTimeout(longPress); longPress = null; };
function cell(touch: Touch) {
  const rect = container.querySelector('.xterm-screen')?.getBoundingClientRect();
  if (!terminal || !rect || !rect.width || !rect.height) return null;
  return { col: Math.max(0, Math.min(terminal.cols - 1, Math.floor((touch.clientX - rect.left) / (rect.width / terminal.cols)))),
    row: terminal.buffer.active.viewportY + Math.max(0, Math.min(terminal.rows - 1, Math.floor((touch.clientY - rect.top) / (rect.height / terminal.rows)))) };
}
container.addEventListener('touchstart', event => {
  clearLongPress(); anchor = null;
  start = null; scrollRemainder = 0;
  const touch = event.touches[0]; if (!touch || event.touches.length !== 1) return;
  start = { x: touch.clientX, y: touch.clientY }; scrollY = touch.clientY; moved = false;
  const origin = cell(touch);
  const begin = () => { if (origin && terminal) { anchor = origin; terminal.blur(); terminal.select(origin.col, origin.row, 1); } };
  if (selecting) { begin(); event.preventDefault(); } else longPress = setTimeout(begin, 450);
}, { passive: false });
container.addEventListener('touchmove', event => {
  const touch = event.touches[0];
  if (!touch || event.touches.length !== 1) { clearLongPress(); anchor = null; start = null; return; }
  if (start && Math.hypot(touch.clientX - start.x, touch.clientY - start.y) > 8) { moved = true; clearLongPress(); }
  if (!terminal) return;
  if (!anchor) {
    // xterm 6 uses a custom viewport, so native WebView scrolling cannot move its history.
    if (!start || !moved || selecting || historical
      || Math.abs(touch.clientY - start.y) <= Math.abs(touch.clientX - start.x)) return;
    const mouseMode = terminal.modes.mouseTrackingMode;
    const remoteScroll = mouseMode === 'vt200' || mouseMode === 'drag' || mouseMode === 'any';
    if (!remoteScroll && terminal.buffer.active.type !== 'normal') return;
    const height = container.querySelector('.xterm-screen')?.getBoundingClientRect().height;
    if (!height) return;
    event.preventDefault();
    scrollRemainder += (scrollY - touch.clientY) / (height / terminal.rows);
    scrollY = touch.clientY;
    const lines = Math.trunc(scrollRemainder);
    scrollRemainder -= lines;
    if (remoteScroll) {
      // Let xterm encode the application's requested mouse protocol, including after replay.
      // Keep the initial cell so a swipe does not drift into another application panel.
      for (let i = 0; i < Math.min(Math.abs(lines), terminal.rows); i++) {
        terminal.element?.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true,
          clientX: start.x, clientY: start.y, deltaY: Math.sign(lines), deltaMode: WheelEvent.DOM_DELTA_LINE }));
      }
    } else if (lines) terminal.scrollLines(lines);
    return;
  }
  const end = cell(touch); if (!end) return;
  event.preventDefault();
  const a = anchor.row * terminal.cols + anchor.col, b = end.row * terminal.cols + end.col;
  const first = Math.min(a, b);
  terminal.select(first % terminal.cols, Math.floor(first / terminal.cols), Math.abs(a - b) + 1);
}, { passive: false });
container.addEventListener('touchend', event => {
  clearLongPress();
  if (anchor) { event.preventDefault(); anchor = null; }
  else if (start && !moved && !selecting) terminal?.focus();
  start = null;
}, { passive: false });
container.addEventListener('touchcancel', () => { clearLongPress(); anchor = null; start = null; });
document.addEventListener('click', event => { if (event.target instanceof Element && event.target.closest('a')) event.preventDefault(); }, true);
send({ type: 'ready', viewId });
