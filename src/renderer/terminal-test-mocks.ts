import { beforeEach, vi } from 'vitest';

const terminalMocks = vi.hoisted(() => ({ terminals: [] as any[], fits: [] as any[] }));
export { terminalMocks };
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    options: any; cols = 80; rows = 24; disposed = false; output = ''; element?: HTMLElement;
    input?: (value: string) => void; osc = new Map<number, (value: string) => boolean>();
    parser = { registerOscHandler: (id: number, handler: (value: string) => boolean) => { this.osc.set(id, handler); return { dispose() {} }; } };
    focus = vi.fn(); reset = vi.fn(() => { this.output = ''; });
    constructor(options: any) { this.options = options; terminalMocks.terminals.push(this); }
    loadAddon(addon: any) { addon.activate(this); }
    onData(listener: (value: string) => void) { this.input = listener; return { dispose() {} }; }
    open(host: HTMLElement) { this.element = document.createElement('pre'); this.element.setAttribute('aria-label', 'Terminal output'); host.append(this.element); this.element.textContent = this.output; }
    write(data: string, done?: () => void) { this.output += data; if (this.element) this.element.textContent = this.output; done?.(); }
    resize(cols: number, rows: number) { this.cols = cols; this.rows = rows; }
    refresh = vi.fn();
    dispose() { this.disposed = true; this.element?.remove(); }
  },
}));
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    terminal: any; activate(terminal: any) { this.terminal = terminal; terminalMocks.fits.push(this); }
    proposeDimensions() { return { cols: 80, rows: 24 }; }
    dispose() {}
  },
}));
vi.stubGlobal('ResizeObserver', class { observe = vi.fn(); disconnect = vi.fn(); unobserve = vi.fn(); });
vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0), 0));
vi.stubGlobal('cancelAnimationFrame', (id: ReturnType<typeof setTimeout>) => clearTimeout(id));
beforeEach(() => {
  terminalMocks.terminals.length = 0; terminalMocks.fits.length = 0;
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 800 });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 480 });
});
