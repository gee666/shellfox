import { expect, it } from 'vitest';
import { parseSshArgs, pickerLines, helpText, sshHelp, remoteCommand } from './presentation';
import type { StoredProfile } from './storage';
const p=(name:string,host:string,user:string,port=22,remoteCwd:string|null=null):StoredProfile=>({id:name,name,host,user,port,remoteCwd,password:null,keyFile:null,source:'manual'});
it('accepts the three profile forms and rejects removed/ambiguous options',()=>{
 expect(parseSshArgs([])).toEqual({});expect(parseSshArgs(['--help'])).toEqual({help:true});
 for(const args of [['prod'],['--profile','prod'],['-p','prod']])expect(parseSshArgs(args)).toEqual({profile:'prod'});
 for(const args of [['--profile'],['--unknown'],['a','b'],['--help','a'],['-p',''],['--persist'],['prod','--persist']])expect(()=>parseSshArgs(args)).toThrow();
});
it('matches the compact picker design and removes color without a TTY',()=>{
 const profiles=[p('prod-web','10.0.0.5','deploy',22,'/var/www'),p('staging','stage.example.com','root',2222),p('nas','192.168.1.20','admin')];
 const plain=pickerLines(profiles,0,false).join('\n');expect(plain).toBe('\n  ssh\n  › prod-web  deploy@10.0.0.5              /var/www\n    staging   root@stage.example.com:2222\n    nas       admin@192.168.1.20         \n\n  ↑↓ select  enter connect  esc cancel');
 expect(plain).not.toContain('\x1b');expect(pickerLines(profiles,1,true).join('\n')).toContain('\x1b[38;2;236;72;153m› staging');
});
it('keeps main help and four-line SSH help',()=>{
 expect(helpText('0.4.0',false)).toBe('\n  shellfox 0.4.0  terminal manager\n\n  usage\n    shellfox                       open shellfox\n    shellfox start [path]          new session in path (default: current folder)\n    shellfox ssh [--profile name]  connect to a saved ssh connection\n    shellfox update [--check]      install the latest release\n\n  examples\n    shellfox start .\n    shellfox ssh --profile prod-web\n\n');
 expect(helpText('0.4.0',false,false)).not.toContain('open shellfox');expect(sshHelp(false).split('\n')).toHaveLength(5);
});
it('quotes remote directories, expands only a leading home prefix, and runs a plain shell',()=>{
 expect(remoteCommand("/srv/it's $(touch /oops)")).toContain("cd -- '/srv/it'\\''s $(touch /oops)'");
 expect(remoteCommand('~/a b')).toContain('cd -- "$HOME"/\'a b\'');expect(remoteCommand('~')).toContain('cd -- "$HOME"');
 expect(remoteCommand('/missing')).toContain('exec "${SHELL:-/bin/sh}" -il');expect(remoteCommand(null)).toBe('exec "${SHELL:-/bin/sh}" -il');
});
