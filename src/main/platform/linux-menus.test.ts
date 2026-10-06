import { expect, it } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, readFile, writeFile, rm, stat } from 'node:fs/promises';
import { LinuxCliIntegration } from './linux-cli';
import { LinuxFileMenus, thunarActions, nautilusExtension, nemoAction, dolphinMenu, fileManagerScript } from './linux-menus';
it('generates both Nautilus API variants and safe local argv for every supported manager',()=>{
 const extension=nautilusExtension('/home/a space/shellfox');expect(extension).toContain("require_version('Nautilus', '4.0')");expect(extension).toContain("require_version('Nautilus', '3.0')");expect(extension).toContain('def get_file_items(self, *args)');expect(extension).toContain('start_new_session=True');expect(extension).not.toContain('shell=True');expect(extension).toContain("raw.lower().startswith('file://')");expect(extension).toContain("value.startswith('/')");
 expect(nemoAction('/app')).toContain('Selection=s');expect(nemoAction('/app',true)).toContain('Selection=none');expect(nemoAction('/app',true)).toContain('start %P');
 expect(dolphinMenu('/app')).toContain('MimeType=inode/directory;');expect(dolphinMenu('/app')).toContain('X-KDE-Protocols=file');expect(nemoAction('/app')).toContain('UriScheme=file');expect(fileManagerScript('/app','CAJA')).toContain('CAJA_SCRIPT_CURRENT_URI');
});
it('escapes launcher names through both key-file and Exec-argv parsing layers',()=>{
 const launcher='/home/a $literal `ticks`/shellfox';
 for(const content of [dolphinMenu(launcher),nemoAction(launcher)]) {
  expect(content).toContain('\\\\$literal');
  expect(content).toContain('\\\\`ticks\\\\`');
 }
});
it('merges/removes only its Thunar action and refuses malformed/foreign collisions',()=>{
 const source='<?xml version="1.0"?><actions><!-- foreign --><action><unique-id>foreign</unique-id><name>Custom &amp; safe</name><command>keep %f</command></action></actions>';
 const merged=thunarActions(source,'/home/a space/shellfox',true);expect(merged).toContain('Custom &amp; safe');expect(merged).toContain('foreign');expect(merged).toContain('shellfox-linux-v1');
 const twice=thunarActions(merged,'/home/a space/shellfox',true);expect(twice.match(/<unique-id>shellfox-linux-v1/g)).toHaveLength(1);
 const removed=thunarActions(twice,'/app',false);expect(removed).toContain('keep %f');expect(removed).not.toContain('shellfox-linux-v1');
 expect(()=>thunarActions('<actions><action><unique-id>shellfox-linux-v1</unique-id></action></actions>','/app',true)).toThrow('Foreign');
 expect(()=>thunarActions('<broken>','/app',true)).toThrow();
});
it('keeps unowned Thunar XML byte-for-byte and requires an action ownership comment',()=>{
 const foreign='<actions>\n  <!-- preserve -->\n  <action><unique-id>other</unique-id></action>\n</actions>\n';
 expect(thunarActions(foreign,'/app',false)).toBe(foreign);
 const collision='<actions><action><unique-id>shellfox-linux-v1</unique-id><description>Documentation mentions Shellfox/linux-v1</description></action></actions>';
 expect(()=>thunarActions(collision,'/app',true)).toThrow('Foreign');
 expect(thunarActions(collision,'/app',false)).toBe(collision);
});
it('does not claim or change foreign menu files that mention the marker',async()=>{
 await mkdir(path.resolve('tmp'),{recursive:true});const home=await mkdtemp(path.resolve('tmp/linux-marker-'));
 const cli=new LinuxCliIntegration({executable:'/fake/app',home}),menus=new LinuxFileMenus(cli,{home,env:{},managers:['nemo']});
 try {
  const file=path.join(home,'.local/share/nemo/actions/shellfox.nemo_action');
  await mkdir(path.dirname(file),{recursive:true});const content='# Instructions refer to Shellfox/linux-v1\n[Nemo Action]\nName=Keep me\n';await writeFile(file,content);
  expect(await menus.set(true)).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});
  expect(await menus.set(false)).toMatchObject({ok:true});
  expect(await readFile(file,'utf8')).toBe(content);
  const xml=path.join(home,'.config/Thunar/uca.xml');await mkdir(path.dirname(xml),{recursive:true});await writeFile(xml,'<actions>  <!-- unchanged --> </actions>\n');
  const before=await stat(xml);await menus.set(false);
  expect((await stat(xml)).mtimeMs).toBe(before.mtimeMs);
 }finally{await rm(home,{recursive:true,force:true});}
});
it('installs for all present managers, returns Nautilus guidance and preserves foreign files',async()=>{
 await mkdir(path.resolve('tmp'),{recursive:true});const home=await mkdtemp(path.resolve('tmp/linux-menus-'));
 const cli=new LinuxCliIntegration({executable:'/fake/app',home}),menus=new LinuxFileMenus(cli,{home,env:{},managers:['nautilus','nemo','dolphin','thunar','caja'],nautilusPython:false});
 try {
  const foreign=path.join(home,'.config/Thunar/uca.xml');await mkdir(path.dirname(foreign),{recursive:true});await writeFile(foreign,'<actions><action><unique-id>other</unique-id><name>Keep me</name></action></actions>');
  expect(await menus.set(true)).toMatchObject({ok:true,value:{installed:true,reason:expect.stringContaining('sudo apt install python3-nautilus')}});
  for(const relative of ['.local/share/nautilus-python/extensions/shellfox.py','.local/share/nautilus/scripts/Open in Shellfox','.local/share/nemo/actions/shellfox.nemo_action','.local/share/nemo/actions/shellfox-background.nemo_action','.local/share/kio/servicemenus/shellfox.desktop','.local/share/kservices5/ServiceMenus/shellfox.desktop','.config/caja/scripts/Open in Shellfox'])expect(await readFile(path.join(home,relative),'utf8')).toContain('Shellfox/linux-v1');
  expect(await menus.set(false)).toMatchObject({ok:true,value:{installed:false}});expect(await readFile(foreign,'utf8')).toContain('Keep me');
  const conflict=path.join(home,'.local/share/nemo/actions/shellfox.nemo_action');await writeFile(conflict,'foreign');expect(await menus.set(true)).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});expect(await readFile(conflict,'utf8')).toBe('foreign');
 }finally{await rm(home,{recursive:true,force:true});}
});
