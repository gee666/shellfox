import path from 'node:path';
import { homedir } from 'node:os';
import { readFile, writeFile, mkdir, chmod, unlink, access, lstat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { execFile } from 'node:child_process';
import { DOMParser, XMLSerializer } from '@xmldom/xmldom';
import type { ExplorerPort } from './explorer';
import type { ExplorerIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { LinuxCliIntegration, LINUX_OWNER, shQuote, ownsLinuxFile } from './linux-cli';
export type FileManager = 'nautilus'|'nemo'|'dolphin'|'thunar'|'caja';
const managers:FileManager[]=['nautilus','nemo','dolphin','thunar','caja'];
const ACTION_ID='shellfox-linux-v1';
const pythonString=(text:string)=>JSON.stringify(text);
export function nautilusExtension(launcher:string):string {
  return `# ${LINUX_OWNER}
import gi
try:
    gi.require_version('Nautilus', '4.0')
except ValueError:
    gi.require_version('Nautilus', '3.0')
from gi.repository import Nautilus, GObject
from urllib.parse import urlparse, unquote
import subprocess
import os
LAUNCHER = ${pythonString(launcher)}
def local_path(item):
    if item is None:
        return None
    raw = item.get_uri()
    uri = urlparse(raw)
    if not raw.lower().startswith('file://') or uri.scheme != 'file' or uri.netloc not in ('', 'localhost') or uri.query or uri.fragment:
        return None
    value = unquote(uri.path)
    return value if value.startswith('/') and not any(ord(c) < 32 or ord(c) == 127 for c in value) and os.path.isdir(value) else None
class ShellfoxMenu(GObject.GObject, Nautilus.MenuProvider):
    def menu(self, item):
        value = local_path(item)
        if not value:
            return []
        action = Nautilus.MenuItem(name='Shellfox::Open', label='Open in Shellfox', tip='Start a terminal session in this folder')
        action.connect('activate', lambda _item: subprocess.Popen([LAUNCHER, 'start', value], start_new_session=True))
        return [action]
    # API 3 passes window,files; API 4 passes files. Both use the last argument.
    def get_file_items(self, *args):
        files = args[-1] if args else []
        return self.menu(files[0]) if files and len(files) == 1 and files[0].is_directory() else []
    def get_background_items(self, *args):
        return self.menu(args[-1]) if args else []
`;
}
export function fileManagerScript(launcher:string, prefix:'NAUTILUS'|'CAJA'):string {
  return `#!/usr/bin/env python3
# ${LINUX_OWNER}
import os, subprocess, sys
from urllib.parse import urlparse, unquote
selected = [p for p in os.environ.get('${prefix}_SCRIPT_SELECTED_FILE_PATHS', '').splitlines() if p]
if len(selected) > 1: sys.exit(1)
folder = selected[0] if selected else ''
if not folder:
    raw = os.environ.get('${prefix}_SCRIPT_CURRENT_URI', '')
    uri = urlparse(raw)
    if not raw.lower().startswith('file://') or uri.scheme != 'file' or uri.netloc not in ('', 'localhost') or uri.query or uri.fragment: sys.exit(1)
    folder = unquote(uri.path)
if not os.path.isabs(folder) or any(ord(c) < 32 or ord(c) == 127 for c in folder) or not os.path.isdir(folder): sys.exit(1)
subprocess.Popen([${pythonString(launcher)}, 'start', folder], start_new_session=True)
`;
}
const desktopQuote=(s:string)=>{
  const quoted='"'+s.replace(/(["\\`$])/g,'\\$1').replaceAll('%','%%')+'"';
  // Desktop/Nemo key files unescape values before parsing the Exec argument list.
  return quoted.replaceAll('\\','\\\\').replaceAll('\n','\\n').replaceAll('\r','\\r').replaceAll('\t','\\t');
};
export function dolphinMenu(launcher:string):string {return `# ${LINUX_OWNER}
[Desktop Entry]
Type=Service
MimeType=inode/directory;
X-KDE-ServiceTypes=KonqPopupMenu/Plugin
Actions=Shellfox;
X-KDE-Protocols=file
X-KDE-Priority=TopLevel
[Desktop Action Shellfox]
Name=Open in Shellfox
Icon=utilities-terminal
Exec=${desktopQuote(launcher)} start %f
`;}
export function nemoAction(launcher:string, background=false):string {return `# ${LINUX_OWNER}
[Nemo Action]
Name=Open in Shellfox
Comment=Start a terminal session in this folder
Exec=${desktopQuote(launcher)} start ${background?'%P':'%F'}
Selection=${background?'none':'s'}
UriScheme=file
Extensions=${background?'any':'dir'};
Icon-Name=utilities-terminal
`;}
const ownsThunarAction = (element: import('@xmldom/xmldom').Element) => Array.from(element.childNodes).some(node => node.nodeType === 8 && node.nodeValue?.trim() === LINUX_OWNER);
/** Parse/merge only our action. Unrelated actions, settings and comments survive. */
export function thunarActions(source:string|null, launcher:string, installed:boolean):string {
  if(!installed && source && !source.includes(ACTION_ID)) return source;
  if(source&&(/<!DOCTYPE|<!ENTITY/i.test(source)||source.length>1024*1024))throw new Error('Unsupported Thunar XML; it was not changed.');
  const doc=new DOMParser({onError:(_level,message)=>{throw new Error(message);}}).parseFromString(source??'<actions/>','text/xml');
  const root=doc.documentElement;if(!root||root.tagName!=='actions')throw new Error('Invalid Thunar actions; they were not changed.');
  let removed = false;
  for(const action of Array.from(root.childNodes))if(action.nodeType===1){const element=action as import('@xmldom/xmldom').Element;if(element.tagName==='action' && element.getElementsByTagName('unique-id')[0]?.textContent===ACTION_ID){
    if(!ownsThunarAction(element)){if(installed)throw new Error('Foreign Thunar action with Shellfox id.');continue;}
    root.removeChild(action); removed = true;
  }}
  if(!installed && !removed) return source ?? '<actions/>';
  if(installed){const action=doc.createElement('action');action.appendChild(doc.createComment(' '+LINUX_OWNER+' '));
    const values:{[key:string]:string}={icon:'utilities-terminal',name:'Open in Shellfox','unique-id':ACTION_ID,command:shQuote(launcher)+' start %f',description:LINUX_OWNER+' — Open a terminal session',patterns:'*',directories:''};
    for(const [name,value] of Object.entries(values)){const child=doc.createElement(name);child.appendChild(doc.createTextNode(value));action.appendChild(child);}root.appendChild(action);
  }
  return new XMLSerializer().serializeToString(doc);
}
interface MenuFile { file:string; content:string; executable?:boolean; xml?:boolean }
export interface LinuxMenuOptions { home?:string; env?:NodeJS.ProcessEnv; managers?:FileManager[]; nautilusPython?:boolean; exists?:(file:string)=>Promise<boolean> }
export class LinuxFileMenus implements ExplorerPort {
  private home:string;private env:NodeJS.ProcessEnv;
  constructor(private readonly cli:LinuxCliIntegration,private readonly options:LinuxMenuOptions={}){this.home=options.home??homedir();this.env=options.env??process.env;}
  private async exists(file:string):Promise<boolean>{if(this.options.exists)return this.options.exists(file);try{await access(file,constants.F_OK);return true;}catch{return false;}}
  async detect():Promise<FileManager[]> {
    if(this.options.managers)return this.options.managers;
    const found:FileManager[]=[];
    for(const manager of managers){const names=manager==='thunar'?['thunar','Thunar']:[manager];const candidates=(this.env.PATH??'').split(':').filter(Boolean).flatMap(p=>names.map(name=>path.join(p,name)));const dataDirs=(this.env.XDG_DATA_DIRS??'/usr/local/share:/usr/share').split(':').filter(Boolean);const data=dataDirs.flatMap(p=>names.map(name=>path.join(p,name)));if((await Promise.all([...candidates,...data].map(p=>this.exists(p)))).some(Boolean))found.push(manager);}
    return found;
  }
  private async hasNautilusPython():Promise<boolean>{if(this.options.nautilusPython!==undefined)return this.options.nautilusPython;return new Promise(resolve=>execFile('/usr/bin/dpkg-query',['-W','-f=${Status}','python3-nautilus'],{timeout:3000,encoding:'utf8',maxBuffer:4096},(error,stdout)=>resolve(!error&&stdout.includes('install ok installed'))));}
  private async files(found:FileManager[]):Promise<MenuFile[]> {
    const data=this.env.XDG_DATA_HOME??path.join(this.home,'.local/share'), config=this.env.XDG_CONFIG_HOME??path.join(this.home,'.config'), launcher=this.cli.launcher, result:MenuFile[]=[];
    for(const manager of found){
      if(manager==='nautilus'){result.push({file:path.join(data,'nautilus-python/extensions/shellfox.py'),content:nautilusExtension(launcher)});if(!await this.hasNautilusPython())result.push({file:path.join(data,'nautilus/scripts/Open in Shellfox'),content:fileManagerScript(launcher,'NAUTILUS'),executable:true});}
      if(manager==='nemo')for(const background of [false,true])result.push({file:path.join(data,'nemo/actions/shellfox'+(background?'-background':'')+'.nemo_action'),content:nemoAction(launcher,background)});
      if(manager==='dolphin')for(const directory of ['kio/servicemenus','kservices5/ServiceMenus'])result.push({file:path.join(data,directory,'shellfox.desktop'),content:dolphinMenu(launcher),executable:true});
      if(manager==='caja')result.push({file:path.join(config,'caja/scripts/Open in Shellfox'),content:fileManagerScript(launcher,'CAJA'),executable:true});
      if(manager==='thunar')result.push({file:path.join(config,'Thunar/uca.xml'),content:'',xml:true});
    }
    return result;
  }
  private async status(found:FileManager[],installed:boolean):Promise<ExplorerIntegrationDto>{const reasons:string[]=[];if(found.includes('nautilus')){if(!await this.hasNautilusPython())reasons.push('For a top-level menu item install python3-nautilus: sudo apt install python3-nautilus');reasons.push('Restart Files to see the menu item (or log out and in)');}if(!found.length)reasons.push('No supported file manager found: Files, Nemo, Dolphin, Thunar or Caja.');return {supported:found.length>0,installed,folderItemInstalled:installed,backgroundInstalled:installed,reason:reasons.join('. ')||null};}
  async get():Promise<Result<ExplorerIntegrationDto>> {try{const found=await this.detect(),files=await this.files(found);let installed=files.length>0;for(const item of files){const current=await readFile(item.file,'utf8').catch(()=>null);if(item.xml){if(!current?.includes(ACTION_ID)||!current.includes(LINUX_OWNER))installed=false;else { const doc=new DOMParser().parseFromString(current,'text/xml'); const action=Array.from(doc.getElementsByTagName('action')).find(action=>action.getElementsByTagName('unique-id')[0]?.textContent===ACTION_ID); if(!action || !ownsThunarAction(action) || action.getElementsByTagName('command')[0]?.textContent!==shQuote(this.cli.launcher)+' start %f')installed=false; }}else if(current!==item.content)installed=false;if(item.executable)try{await access(item.file,constants.X_OK);}catch{installed=false;}}const cli=await this.cli.get();installed=installed&&cli.ok&&cli.value.installed;return success(await this.status(found,installed));}catch{return failure('STORAGE_FAILED','File-manager integration could not be inspected.',true);}}
  async set(installed:boolean):Promise<Result<ExplorerIntegrationDto>> {try{
    const found=await this.detect();if(installed&&!found.length)return failure('UNSUPPORTED','No supported file manager is present.');
    // Disable also checks managers no longer installed; only owned files are removed.
    const files=await this.files(installed?found:managers), changes:{item:MenuFile;content:string|null}[]=[];
    for(const item of files){const metadata=await lstat(item.file).catch(error=>{if(error.code==='ENOENT')return null;throw error;});if(metadata?.isSymbolicLink()){if(installed)return failure('AUTH_FAILED','A file-manager action is a foreign symbolic link: '+item.file);continue;}const old=await readFile(item.file,'utf8').catch(error=>{if(error.code==='ENOENT')return null;throw error;});
      if(item.xml){if(!old&&!installed)continue;const content=thunarActions(old,this.cli.launcher,installed);if(content!==old)changes.push({item,content});}
      else{if(old!==null&&!ownsLinuxFile(old)){if(installed)return failure('AUTH_FAILED','A file-manager action at '+item.file+' is not owned by Shellfox.');continue;}changes.push({item,content:installed?item.content:null});}
    }
    if(installed){const cli=await this.cli.set(true);if(!cli.ok)return cli;}
    for(const {item,content} of changes){if(content===null)await unlink(item.file).catch(error=>{if(error.code!=='ENOENT')throw error;});else{await mkdir(path.dirname(item.file),{recursive:true});await writeFile(item.file,content);if(item.executable)await chmod(item.file,0o755);}}
    // Always remove a previously installed owned Nautilus fallback on disable.
    if(!installed){const fallback=path.join(this.env.XDG_DATA_HOME??path.join(this.home,'.local/share'),'nautilus/scripts/Open in Shellfox');const old=await readFile(fallback,'utf8').catch(()=>null);if(old && ownsLinuxFile(old) && !(await lstat(fallback)).isSymbolicLink())await unlink(fallback);}
    return installed?this.get():success(await this.status(found,false));
  }catch(error){return failure('STORAGE_FAILED','File-manager integration could not be completed: '+(error as Error).message,true);}}
}
