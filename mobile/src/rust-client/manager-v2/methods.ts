import { z } from 'zod';
import { remoteMethods } from '../../../../src/shared/remote-schemas';
import { observerRequestSchemas, observerValueSchemas, type ObserverPayload } from '../observer-v2/methods';
import { reply, type ManagerError } from './schemas';
export const nativeOnlyMethods=['attachTerminal','takeTerminalControl','detachTerminal','writeTerminal','resizeTerminal','acknowledgeTerminal','beginTerminalUpload','writeTerminalUpload','finishTerminalUpload','cancelTerminalUpload'] as const;
export type NativeOnlyMethod=typeof nativeOnlyMethods[number];
export type MetadataMethod=Exclude<typeof remoteMethods[number],NativeOnlyMethod>;
export type ManagerPayload<M extends MetadataMethod>=ObserverPayload<M>;
export type ManagerResponse<M extends MetadataMethod>=
  |{version:2;ok:true;value:z.output<(typeof observerValueSchemas)[M]>}
  |{version:2;ok:false;error:ManagerError};
const values:{[M in MetadataMethod]:{parse(raw:unknown):z.output<(typeof observerValueSchemas)[M]>}}=observerValueSchemas;
export function parseResponse<M extends MetadataMethod>(method:M,raw:unknown):ManagerResponse<M>{
 const result=reply(z.unknown()).parse(raw);if(!result.ok)return result;
 return {version:2,ok:true,value:values[method].parse(result.value)};
}
export function command<M extends MetadataMethod>(method:M,payload:ManagerPayload<M>):string{
 if(new Set<string>(nativeOnlyMethods).has(method))throw new Error('Terminal authority belongs to native view bindings.');
 return encoded(method,observerRequestSchemas[method].parse(payload));
}
export function encoded(method:string,payload:unknown):string{
 if(!new Set<string>(remoteMethods).has(method))throw new Error('Unknown manager method.');
 const text=JSON.stringify({method,payload});if(new TextEncoder().encode(text).byteLength>1048576)throw new Error('Manager request exceeds limit.');return text;
}
export { reply };
