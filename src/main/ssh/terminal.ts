import { emitKeypressEvents } from 'node:readline';
import type { StoredProfile } from './storage';
import { pickerLines } from './presentation';
export class Cancelled extends Error {}
interface Key { name?: string; ctrl?: boolean; sequence?: string }
export class CliTerminal {
  readonly color = process.stdout.isTTY === true && process.env.NO_COLOR === undefined;
  readonly tty = process.stdin.isTTY === true && process.stdout.isTTY === true;
  readonly abort = new AbortController();
  private raw = false;
  private started = false;
  private signal = () => this.abort.abort();
  private session = false;
  private cancel = (_: string, k: Key) => { if (!this.session && k.ctrl && k.name === 'c') this.abort.abort(); };
  start() {
    if (!this.tty) return;
    this.started = true;
    this.raw = process.stdin.isRaw;
    emitKeypressEvents(process.stdin); process.stdin.setRawMode(true); process.stdin.resume();
    process.stdin.on('keypress',this.cancel);
    for(const name of ['SIGINT','SIGTERM','SIGHUP'] as const)process.on(name,this.signal);
  }
  remote() { this.session = true; }
  restore() {
    if(!this.started){process.stdin.pause();return;}
    this.started=false;
    process.stdin.off('keypress',this.cancel);
    for(const name of ['SIGINT','SIGTERM','SIGHUP'] as const)process.off(name,this.signal);
    if (this.tty) { process.stdin.setRawMode(this.raw); process.stdout.write('\x1b[0m\x1b[?25h'); }
    process.stdin.pause();
  }
  prompt(label: string, hidden = false, signal?: AbortSignal): Promise<string> {
    if (!this.tty) return Promise.reject(new Error('an interactive terminal is required for login or host trust'));
    if (this.abort.signal.aborted || signal?.aborted) return Promise.reject(new Cancelled());
    process.stdout.write('  ' + label);
    return new Promise((resolve,reject) => {
      let value = '';
      const cleanup = () => { process.stdin.off('keypress',key); this.abort.signal.removeEventListener('abort',cancel); signal?.removeEventListener('abort',cancel); };
      const cancel = () => { cleanup(); process.stdout.write('\n'); reject(new Cancelled()); };
      const key = (text: string, k: Key) => {
        if (k.ctrl && k.name === 'c') return;
        if (k.name === 'return' || k.name === 'enter') { cleanup(); process.stdout.write('\n'); resolve(value); }
        else if (k.name === 'backspace') { if(value) { value=Array.from(value).slice(0,-1).join(''); if(!hidden)process.stdout.write('\b \b'); } }
        else if (k.ctrl && k.name === 'd') cancel();
        else if (text && !k.ctrl && !/[\x00-\x1f\x7f]/.test(text)) { value+=text; if(!hidden)process.stdout.write(text); }
      };
      process.stdin.on('keypress',key); this.abort.signal.addEventListener('abort',cancel,{once:true}); signal?.addEventListener('abort',cancel,{once:true});
    });
  }
  pick(profiles: StoredProfile[]): Promise<StoredProfile> {
    return new Promise((resolve,reject) => {
      let selected=0, lines=0;
      const erase = () => { if(lines)process.stdout.write(`\x1b[${lines}A\r\x1b[J`); };
      const draw = () => { erase(); const capacity=Math.max(1,(process.stdout.rows||24)-6),start=Math.max(0,Math.min(selected-Math.floor(capacity/2),profiles.length-capacity));const rendered=pickerLines(profiles.slice(start,start+capacity),selected-start,this.color,process.stdout.columns); lines=rendered.length; process.stdout.write(rendered.join('\n')+'\n'); };
      const cleanup = () => { process.stdin.off('keypress',key); process.stdout.off('resize',draw); this.abort.signal.removeEventListener('abort',cancel); erase(); };
      const cancel = () => { cleanup(); reject(new Cancelled()); };
      const key = (text: string,k: Key) => {
        if(k.name==='escape'||text==='q')cancel();
        else if(k.name==='up'||text==='k'){selected=(selected+profiles.length-1)%profiles.length;draw();}
        else if(k.name==='down'||text==='j'){selected=(selected+1)%profiles.length;draw();}
        else if(k.name==='return'||k.name==='enter'){cleanup();resolve(profiles[selected]);}
        else if(/^[1-9]$/.test(text)&&Number(text)<=profiles.length){cleanup();resolve(profiles[Number(text)-1]);}
      };
      process.stdin.on('keypress',key); process.stdout.on('resize',draw); this.abort.signal.addEventListener('abort',cancel,{once:true}); draw();
    });
  }
}
