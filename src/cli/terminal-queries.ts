const MAX_CONTROL_LENGTH = 4096;
type State = 'text' | 'escape' | 'csi' | 'osc' | 'dcs';
const stringHeader = /^(?:\x1b\]|\x1bP|\u009d|\u0090)/;
const ignoredStringControls = /[\x00-\x17\x19\x1c-\x1f\x7f]/g;

/** Match the persistent host parser's query ownership, not the external display. */
function filterSequence(state: State, sequence: string, historical: boolean): string {
  if (state === 'csi') {
    const body = sequence.replace(/^(?:\x1b\[|\u009b)/, '');
    const match = /^([?>=]?)([\d;:]*)([ -/]*)([@-~])$/.exec(body);
    if (!match) return sequence;
    const [, prefix, params, intermediate, final] = match;
    const first = Number(params.split(/[;:]/)[0] || 0);
    // ConPTY's direct transport may request Win32 key packets. Passing that
    // mode to a nested terminal changes the CLI's own keyboard encoding.
    if ((final === 'h' || final === 'l') && prefix === '?' && !intermediate && params.split(';').includes('9001')) {
      const modes = params.split(';').filter(mode => mode !== '9001');
      return modes.length ? (sequence.startsWith('\u009b') ? '\u009b' : '\x1b[') + '?' + modes.join(';') + final : '';
    }
    // The external window owns its geometry. Never feed a remote resize back
    // into the caller's resize event and then into the remote PTY again.
    if (final === 't' && !prefix && !intermediate && (first === 4 || first === 8)) return '';
    if (final === 'c' && !intermediate && (prefix === '' || prefix === '>')) return '';
    if (final === 'n' && !intermediate && (prefix === '' && (first === 5 || first === 6) || prefix === '?' && first === 6)) return '';
    if (final === 'p' && intermediate === '$' && (prefix === '' || prefix === '?')) return '';
    return final === 't' && !prefix && !intermediate && (first === 18 || historical && (first === 14 || first === 16)) ? '' : sequence;
  }
  if (state !== 'osc' || !historical) return sequence;
  const ending = sequence.endsWith('\x1b\\') ? '\x1b\\' : sequence.slice(-1);
  const body = sequence.replace(stringHeader, '').slice(0, -ending.length).replace(ignoredStringControls, '');
  const [id, ...values] = body.split(';');
  if (!['4', '10', '11', '12'].includes(id) || !values.includes('?')) return sequence;
  const header = sequence.startsWith('\u009d') ? '\u009d' : '\x1b]';
  if (id === '4') {
    const settings: string[] = [];
    for (let i = 0; i < values.length; i += 2) {
      if (values[i + 1] !== '?') settings.push(...values.slice(i, i + 2));
    }
    return settings.length ? header + '4;' + settings.join(';') + ending : '';
  }
  // OSC 10 may set foreground, background and cursor in one command. Removing
  // a query must not shift the remaining settings onto different color slots.
  return values.map((value, index) => value === '?' ? '' : header + (Number(id) + index) + ';' + value + ending).join('');
}

/** Streaming VT filter. Bounded unknown strings pass through; known queries never do. */
export class TerminalQueryFilter {
  private state: State = 'text';
  private pending = '';
  private historical = false;
  private escape = false;
  private passthrough = false;
  private discard = false;

  reset(): void { this.state = 'text'; this.pending = ''; this.escape = false; this.passthrough = false; this.discard = false; }
  push(data: string, historical: boolean): string {
    let output = '';
    for (const char of data) output += this.accept(char, historical);
    return output;
  }
  private accept(char: string, historical: boolean): string {
    if (this.state !== 'text' && (char === '\x18' || char === '\x1a')) { this.reset(); return char; }
    if (this.state === 'text') {
      const state = char === '\x1b' ? 'escape' : char === '\u009b' ? 'csi' : char === '\u009d' ? 'osc' : char === '\u0090' ? 'dcs' : null;
      if (!state) return char;
      this.state = state; this.pending = char; this.historical = historical;
      return '';
    }
    if (this.state === 'escape' || this.state === 'csi') {
      if (char === '\x1b') { this.reset(); this.state = 'escape'; this.pending = char; this.historical = historical; return ''; }
      // C0 executes without becoming part of a CSI parameter/header. DEL is ignored.
      if (char < ' ' || char === '\x7f') return char;
      if (char === '\u009b') { this.reset(); this.state = 'csi'; this.pending = char; this.historical = historical; return ''; }
      this.pending += char;
      if (this.state === 'escape') {
        if (char === '[') this.state = 'csi';
        else if (char === ']') this.state = 'osc';
        else if (char === 'P') this.state = 'dcs';
        else { const output = this.pending; this.reset(); return output; }
      } else if (char >= '@' && char <= '~') {
        const output = filterSequence(this.state, this.pending, this.historical); this.reset(); return output;
      } else if (this.pending.length > MAX_CONTROL_LENGTH) { const output = this.pending; this.reset(); return output; }
      return '';
    }
    if (this.escape) {
      this.escape = false;
      if (char === '\\') return this.endString('\x1b\\');
      // ESC restarts VT parsing unless it completes ST. Cancel an already
      // streamed unknown string before suppressing any following query.
      const cancel = this.passthrough ? '\x18' : '';
      this.reset(); this.state = 'escape'; this.pending = '\x1b'; this.historical = historical;
      return cancel + this.accept(char, historical);
    }
    if (char === '\x1b') { this.escape = true; return ''; }
    if (char === '\u009c' || this.state === 'osc' && char === '\x07') return this.endString(char);
    if (this.discard) return '';
    if (this.passthrough) return char;
    this.pending += char;
    if (this.state === 'dcs' && /^[\d;]*\$q$/.test(this.pending.replace(stringHeader, '').replace(ignoredStringControls, ''))) {
      // DECRQSS payloads may be arbitrarily long. Identify the header now and
      // discard through ST/cancellation without buffering or leaking the query.
      this.discard = true; this.pending = ''; return '';
    }
    if (this.pending.length > MAX_CONTROL_LENGTH) {
      const output = this.pending; this.pending = ''; this.passthrough = true; return output;
    }
    return '';
  }
  private endString(ending: string): string {
    const output = this.discard ? '' : this.passthrough ? ending : filterSequence(this.state, this.pending + ending, this.historical);
    this.reset(); return output;
  }
}
