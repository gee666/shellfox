import { expect, it, vi } from 'vitest';
import { resolvePython } from './python';
import { PIDFD_READY } from './pidfd-preflight';
it('prefers configured executable, then system Python, then PATH without shell parsing', async()=>{
 const run=vi.fn(async()=>Buffer.from(PIDFD_READY)),exists=vi.fn(async()=>true);
 expect((await resolvePython('/opt/my python',{exists,run,env:{PATH:'/custom/bin'}})).detected).toBe('/opt/my python');
 expect(run.mock.calls[0]).toEqual(['/opt/my python',['-c',expect.any(String)]]);
 expect((await resolvePython(null,{exists:async p=>p==='/usr/bin/python3',run,env:{PATH:'/custom/bin'}})).detected).toBe('/usr/bin/python3');
 expect((await resolvePython(null,{exists:async p=>p==='/custom/bin/python3',run,env:{PATH:'/custom/bin'}})).detected).toBe('/custom/bin/python3');
});
it('resolves relative and empty PATH entries against caller cwd after system Python',async()=>{
 const run=vi.fn(async()=>Buffer.from(PIDFD_READY));
 expect((await resolvePython(null,{cwd:'/caller',env:{PATH:'relative tools:/another'},exists:async p=>p==='/caller/relative tools/python3',run})).detected).toBe('/caller/relative tools/python3');
 expect((await resolvePython(null,{cwd:'/caller',env:{PATH:':/another'},exists:async p=>p==='/caller/python3',run})).detected).toBe('/caller/python3');
 expect((await resolvePython(null,{cwd:'/caller',env:{PATH:''},exists:async p=>p==='/usr/bin/python3'||p==='/caller/python3',run})).detected).toBe('/usr/bin/python3');
});
it('rejects configured relative/missing/nonexecutable interpreters and reports failed pidfds',async()=>{
 const run=vi.fn(async()=>Buffer.from('not ready'));
 expect(await resolvePython('python3',{exists:async()=>true,run},true)).toMatchObject({usable:false,detected:null});expect(run).not.toHaveBeenCalled();
 expect(await resolvePython('/missing',{exists:async()=>false,run},true)).toMatchObject({usable:false,detected:null});
 expect(await resolvePython('/bad',{exists:async()=>true,run},true)).toMatchObject({usable:false,reason:expect.stringContaining('pidfd')});
 expect(await resolvePython(null,{exists:async()=>false,env:{PATH:''}})).toMatchObject({reason:'Python 3 not found. Set its path in Settings → Python.'});
});
