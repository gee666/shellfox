import type { PseudoTtyOptions, TerminalModes } from 'ssh2';
// RFC 4254 + RFC 8160. ssh2 accepts an encoded modes Buffer but its named
// opcode table predates IUTF8 (42), so encode it without patching the library.
export const SSH_TTY_MODES = {
  VINTR:[1,3],VQUIT:[2,28],VERASE:[3,127],VKILL:[4,21],VEOF:[5,4],VEOL:[6,255],VEOL2:[7,255],
  VSTART:[8,17],VSTOP:[9,19],VSUSP:[10,26],VREPRINT:[12,18],VWERASE:[13,23],VLNEXT:[14,22],VDISCARD:[18,15],
  IGNPAR:[30,0],INPCK:[32,0],ISTRIP:[33,0],INLCR:[34,0],IGNCR:[35,0],ICRNL:[36,1],IXON:[38,1],IXOFF:[40,0],IUTF8:[42,1],
  ISIG:[50,1],ICANON:[51,1],ECHO:[53,1],ECHOE:[54,1],ECHOK:[55,1],ECHONL:[56,0],NOFLSH:[57,0],IEXTEN:[59,1],ECHOCTL:[60,1],ECHOKE:[61,1],
  OPOST:[70,1],ONLCR:[72,1],OCRNL:[73,0],ONOCR:[74,0],ONLRET:[75,0],CS7:[90,0],CS8:[91,1],PARENB:[92,0],PARODD:[93,0],
  TTY_OP_ISPEED:[128,38400],TTY_OP_OSPEED:[129,38400],
} as const;
export function terminalModes(): Buffer {
  const values=Object.values(SSH_TTY_MODES),bytes=Buffer.alloc(values.length*5+1);
  values.forEach(([opcode,value],i)=>{bytes[i*5]=opcode;bytes.writeUInt32BE(value,i*5+1);});
  return bytes; // zero terminator is already present
}
export function remotePty(cols=80,rows=24): PseudoTtyOptions {
  return {term:'xterm-256color',cols,rows,width:0,height:0,modes:terminalModes() as unknown as TerminalModes};
}
export function remoteEnvironment(env:NodeJS.ProcessEnv=process.env): Record<string,string> {
  const result:Record<string,string>={TERM:'xterm-256color',COLORTERM:'truecolor'};
  for(const [name,value] of Object.entries(env))if((name==='LANG'||/^LC_[A-Z_]+$/.test(name))&&value&&value.length<=128&&/^[a-zA-Z0-9_.@:-]+$/.test(value))result[name]=value;
  // NO_COLOR belongs to Shellfox's own messages, never remote output/env.
  return result;
}
