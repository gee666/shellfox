import { describe, expect, it } from 'vitest';
import { boundWindowSchema, memberRegistrationSchema, sessionWindowEventSchema } from './session-windows';
const id='00000000-0000-4000-8000-000000000001';
const binding={target:{kind:'windows-terminal',windowName:`shellfox-${id}`,hwnd:'123',owner:{pid:1,startTime:'100'},sessionId:id,markerPrefix:`SHELLFOX:${id}:`,verification:'native-title'},generation:id};
const member={registration:{sessionId:id,tabId:id,operationId:id,shell:{pid:2,startTime:'200'},shellExecutable:'C:\\pwsh.exe',cwd:'C:\\test',registeredAt:'2026-10-04T12:00:00Z'},wtSession:id,binding};
describe('session window extension contract',()=>{
 it('requires generation on bound/lost events',()=>{expect(boundWindowSchema.safeParse(binding).success).toBe(true);expect(sessionWindowEventSchema.safeParse({type:'lost',binding,reason:'closed'}).success).toBe(true);expect(boundWindowSchema.safeParse({target:binding.target}).success).toBe(false);});
 it('carries exact shell identity and previous owner, never secrets',()=>{expect(memberRegistrationSchema.safeParse(member).success).toBe(true);expect(sessionWindowEventSchema.safeParse({type:'adopted',member,previous:member,source:'manual'}).success).toBe(true);expect(sessionWindowEventSchema.safeParse({type:'adopted',member,previous:null,source:'manual',token:'forbidden'}).success).toBe(false);});
 it('does not accept a connection ID as a window handle',()=>{expect(boundWindowSchema.safeParse({...binding,target:{...binding.target,hwnd:id}}).success).toBe(false);expect(memberRegistrationSchema.safeParse({...member,wtSession:'not-a-guid'}).success).toBe(false);});
});
