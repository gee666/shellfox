import { Terminal, type ITheme } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { installTerminalQueryHandlers } from './terminal-queries';
import { isWebLink, webLinkProvider } from './terminal-links';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, derivePalette } from './theme';

const DEFAULT_THEME: ITheme = derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND).terminal;
const MAC = /Mac/.test(navigator.userAgent);

export interface TerminalSurface {
  element: HTMLElement;
  write(data: string, done: () => void, historical?: boolean): void;
  reset(): void;
  fit(): { cols: number; rows: number } | null;
  /** Sets the grid before any output is parsed, so a snapshot is replayed at the size it was taken at. */
  resize?(cols: number, rows: number): void;
  focus(): void;
  setInput(enabled: boolean): void;
  /** Pause display-only work while retaining the parser, buffers and device replies. */
  setVisible?(visible: boolean): void;
  setTheme?(theme: ITheme): void;
  dispose(): void;
}
/** Clipboard access goes through the main process; the renderer has no clipboard permissions. */
export interface SurfaceClipboard {
  copy(text: string): void;
  /** Resolves with clipboard text, or null when nothing can be pasted. */
  read(): Promise<string | null>;
  /** Opens an http(s) link in the system browser. */
  openLink?(url: string): void;
}
export type SurfaceFactory = (input: (data: string, userInitiated?: boolean) => void, theme?: ITheme, clipboard?: SurfaceClipboard) => TerminalSurface;
/** Ctrl+Shift+C / Ctrl+Shift+V (KeyboardEvent.code, so it is keyboard-layout independent). */
export function clipboardShortcut(ev: KeyboardEvent): 'copy' | 'paste' | null {
  if (!ev.ctrlKey || !ev.shiftKey || ev.altKey || ev.metaKey) return null;
  return ev.code === 'KeyC' ? 'copy' : ev.code === 'KeyV' ? 'paste' : null;
}
export const createTerminalSurface: SurfaceFactory = (input, theme = DEFAULT_THEME, clipboard) => {
  const element = document.createElement('div');
  // With mouse reporting on, the click also reaches the application, and TUIs
  // such as pi open the link themselves: opening here too launches it twice.
  // The selection modifier bypasses reporting, so it still opens links there.
  const openLink = (url: string, event: MouseEvent) => {
    const bypass = MAC ? event.altKey : event.shiftKey;
    if (terminal.modes.mouseTrackingMode === 'none' || bypass) clipboard?.openLink?.(url);
  };
  element.className = 'terminal-surface';
  const terminal = new Terminal({
    allowProposedApi: true, scrollback: 3000, fontSize: 14,
    fontFamily: "'Cascadia Code', Consolas, 'DejaVu Sans Mono', monospace",
    cursorBlink: false, convertEol: false, disableStdin: true,
    // Preserve application RGB choices; bold must not remap ANSI palette colors.
    minimumContrastRatio: 1, drawBoldTextInBrightColors: false,
    overviewRuler: { width: 8 },
    // Option+click bypasses mouse reporting on macOS, as Shift+click does elsewhere.
    macOptionClickForcesSelection: true,
    theme,
    // OSC 8 hyperlinks: only http(s), opened by the main process in the system browser.
    // OSC 8 text hides its target, so hovering shows it as a tooltip.
    linkHandler: {
      activate: (event, uri) => { if (event.button === 0 && isWebLink(uri)) openLink(uri, event); },
      hover: (_event, uri) => { if (isWebLink(uri)) element.title = uri; },
      leave: () => element.removeAttribute('title'),
      allowNonHttpProtocols: false,
    },
  });
  if (clipboard?.openLink) terminal.registerLinkProvider(webLinkProvider(terminal, openLink, {
    hover: url => { element.title = url; }, leave: () => element.removeAttribute('title'),
  }));
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  // Shell output must not read/write the OS clipboard.
  terminal.parser.registerOscHandler(52, () => true);
  let historical = false;
  const queries = installTerminalQueryHandlers(terminal.parser, () => historical);
  let parsing = false;
  let disposed = false;
  let inputEnabled = false;
  let visible = false;
  let needsRefresh = true;
  let userInput = false;
  // Output parsing emits device replies through onData too. Browser input and
  // explicit clipboard paste remain user operations even during a slow write.
  const markUserInput = () => { userInput = true; queueMicrotask(() => { userInput = false; }); };
  const inputEvents = ['keydown', 'keypress', 'input', 'compositionend', 'paste', 'mousedown', 'mouseup', 'mousemove', 'wheel'];
  inputEvents.forEach(type => element.addEventListener(type, markUserInput, true));
  // xterm owns desktop text input, composition and native paste. This listener
  // only marks encoded keys as user actions; it never edits or cancels input.
  terminal.onKey(markUserInput);
  terminal.onData(data => {
    if (inputEnabled) input(data, userInput || !parsing);
  });
  // xterm 6 treats SGR hover reports as user input and clears local selection.
  // Capture only button-free motion; drag events must still reach selection/TUI handlers.
  const preserveSelection = (ev: MouseEvent) => {
    if (terminal.modes.mouseTrackingMode === 'any' && ev.buttons === 0 && (ev.shiftKey || terminal.hasSelection())) {
      ev.stopPropagation();
    }
  };
  element.addEventListener('mousemove', preserveSelection, true);
  // Middle click with a selection copies it to the clipboard instead of pasting
  // the primary selection or reporting the click to a mouse-enabled app.
  let middleCopyUntil = 0;
  const middleCopy = (ev: MouseEvent) => {
    if (ev.button !== 1) return;
    if (ev.type === 'mousedown') {
      if (!clipboard || !terminal.hasSelection()) { middleCopyUntil = 0; return; }
      clipboard.copy(terminal.getSelection());
      middleCopyUntil = Date.now() + 1000;
    } else if (Date.now() > middleCopyUntil) return;
    ev.preventDefault(); ev.stopPropagation();
  };
  const blockMiddlePaste = (ev: Event) => {
    if (Date.now() <= middleCopyUntil) { middleCopyUntil = 0; ev.preventDefault(); ev.stopPropagation(); }
  };
  const middleEvents = ['mousedown', 'mouseup', 'auxclick'] as const;
  middleEvents.forEach(type => element.addEventListener(type, middleCopy, true));
  element.addEventListener('paste', blockMiddlePaste, true);
  if (clipboard) terminal.attachCustomKeyEventHandler(ev => {
    const action = clipboardShortcut(ev);
    if (!action) return true;
    // Swallow keydown/keypress/keyup so xterm never sends ^C/^V, and stop Chromium's
    // own paste-as-plain-text from firing a second paste event.
    if (ev.type !== 'keydown') return false;
    ev.preventDefault();
    if (action === 'copy') { if (terminal.hasSelection()) clipboard.copy(terminal.getSelection()); }
    // paste() applies bracketed-paste mode and newline normalization, then emits onData.
    else if (inputEnabled) void clipboard.read().then(text => {
      if (text && !disposed && inputEnabled) {
        userInput = true;
        try { terminal.paste(text); }
        finally { userInput = false; }
      }
    });
    return false;
  });
  let opened = false;
  return {
    element,
    write: (data, done, replay = false) => {
      parsing = true; historical = replay;
      terminal.write(data, () => { parsing = false; historical = false; done(); });
    }, reset: () => terminal.reset(),
    fit() {
      if (!element.isConnected || element.clientWidth < 20 || element.clientHeight < 20) return null;
      if (!opened) {
        terminal.open(element); opened = true;
        if (terminal.textarea) terminal.textarea.readOnly = !inputEnabled;
      }
      const size = fit.proposeDimensions();
      if (!size || !Number.isFinite(size.cols) || !Number.isFinite(size.rows)) return null;
      const cols = Math.max(2, Math.min(500, Math.floor(size.cols)));
      const rows = Math.max(2, Math.min(500, Math.floor(size.rows)));
      if (cols !== terminal.cols || rows !== terminal.rows) terminal.resize(cols, rows);
      if (needsRefresh) { terminal.refresh(0, terminal.rows - 1); needsRefresh = false; }
      return { cols, rows };
    },
    resize: (cols, rows) => { if (cols !== terminal.cols || rows !== terminal.rows) terminal.resize(cols, rows); },
    focus: () => { if (opened) terminal.focus(); },
    setInput: enabled => {
      inputEnabled = enabled; terminal.options.disableStdin = !enabled;
      if (terminal.textarea) terminal.textarea.readOnly = !enabled;
    },
    setVisible: next => {
      if (visible === next) return;
      visible = next; terminal.options.cursorBlink = next;
      if (next) needsRefresh = true;
    },
    setTheme: next => { terminal.options.theme = next; },
    dispose: () => {
      disposed = true;
      inputEvents.forEach(type => element.removeEventListener(type, markUserInput, true));
      element.removeEventListener('mousemove', preserveSelection, true);
      middleEvents.forEach(type => element.removeEventListener(type, middleCopy, true));
      element.removeEventListener('paste', blockMiddlePaste, true);
      queries.dispose(); terminal.dispose(); element.remove();
    },
  };
};
