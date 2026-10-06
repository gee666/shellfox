import { expect, it, vi } from 'vitest';
const state=vi.hoisted(()=>({usable:false}));
vi.mock('./python',()=>({resolvePython:async(configured:string|null)=>{state.usable=configured==='/custom/python';return {detected:state.usable?configured:null,usable:state.usable,reason:state.usable?null:'Python 3 not found. Set its path in Settings → Python.'};}}));
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { MemoryRepository, factoryFixture, profiles } from './test-fixtures';
it('rejects unusable Python and refreshes disabled Linux profiles after a successful setting without restarting',async()=>{
 const original=Object.getOwnPropertyDescriptor(process,'platform')!;Object.defineProperty(process,'platform',{value:'linux',configurable:true});
 const repo=new MemoryRepository(),f=factoryFixture(),service=new EmbeddedSessionService(repo,new PtyBackend({...f.options,platform:'linux',discover:async()=>({...profiles,profiles:profiles.profiles.map(p=>({...p,available:state.usable})),defaultProfileId:state.usable?'login-shell':null})}),()=>({setWatch(){},dispose(){}}));
 try{
  await service.initialize();expect(service.probe.available).toBe(false);
  const bad=await service.saveSettings({...repo.settings(),pythonPath:'/missing'});expect(bad).toMatchObject({ok:false,error:{code:'VALIDATION',message:expect.stringContaining("This Python can't be used:")}});expect(repo.settings().pythonPath).toBeNull();
  const good=await service.saveSettings({...repo.settings(),pythonPath:'/custom/python'});expect(good).toMatchObject({ok:true,value:{pythonPath:'/custom/python'}});expect(service.probe.python).toMatchObject({usable:true,detected:'/custom/python'});expect(service.probe.available).toBe(true);expect(service.getTerminalProfiles()).toMatchObject({ok:true,value:{profiles:[expect.objectContaining({available:true}),expect.anything()]}});
 }finally{await service.dispose();Object.defineProperty(process,'platform',original);}
});
