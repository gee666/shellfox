import { expect, it } from 'vitest';
import { remoteEnvironment, remotePty, terminalModes, SSH_TTY_MODES } from './pty';
it('advertises xterm-256color and real character dimensions with unknown pixel sizes zero',()=>{
 const pty=remotePty(132,43);expect(pty).toMatchObject({term:'xterm-256color',cols:132,rows:43,width:0,height:0});expect(Buffer.isBuffer(pty.modes)).toBe(true);
});
it('encodes sane SSH tty modes including RFC 8160 IUTF8 and UTF-8 erase',()=>{
 const bytes=terminalModes(),decoded:Record<number,number>={};
 for(let i=0;i<bytes.length-1;i+=5)decoded[bytes[i]]=bytes.readUInt32BE(i+1);
 expect(bytes.at(-1)).toBe(0);expect(decoded[42]).toBe(1);expect(decoded[3]).toBe(127);expect(decoded[51]).toBe(1);expect(decoded[53]).toBe(1);expect(decoded[36]).toBe(1);expect(decoded[91]).toBe(1);
 expect(Object.keys(decoded)).toHaveLength(Object.keys(SSH_TTY_MODES).length);
});
it('forwards terminal capability and valid locale, not local NO_COLOR or arbitrary secrets',()=>{
 expect(remoteEnvironment({TERM:'dumb',COLORTERM:'',LANG:'C.UTF-8',LC_CTYPE:'en_US.UTF-8',LC_ALL:'bad\nvalue',NO_COLOR:'1',TOKEN:'secret'})).toEqual({TERM:'xterm-256color',COLORTERM:'truecolor',LANG:'C.UTF-8',LC_CTYPE:'en_US.UTF-8'});
 expect(remoteEnvironment({})).toEqual({TERM:'xterm-256color',COLORTERM:'truecolor'});
});
