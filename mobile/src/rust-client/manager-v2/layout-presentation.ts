import type {NativeLayoutPresentation} from '../native-terminal';
import type {UploadIdentity} from '../../terminal/upload';
import type {LayoutComparisonOutcome} from '../../terminal/keyboard-retirement';
import {attachmentSchema,type ManagerAttachment} from './schemas';

type Attached=Extract<ManagerAttachment,{kind:'attached'}>;
type Owned=Attached&{input:NonNullable<Attached['input']>};
export interface LayoutPresentation {
  readonly source:Owned;
  readonly before:Owned;
  readonly identity:UploadIdentity;
  valid:boolean;
}
function owned(value:ManagerAttachment|undefined,reject?:()=>void):value is Owned {
  const matches=value?.kind==='attached'&&value.inputState==='owned'&&value.input!==null;
  if(!matches)reject?.();return matches;
}
function sameControl(a:Owned,b:Owned,reject?:()=>void):boolean {
  const matches=a.input.tabId===b.input.tabId&&a.input.generation===b.input.generation
    &&a.input.controlId===b.input.controlId&&a.input.controlEpoch===b.input.controlEpoch;
  if(!matches)reject?.();return matches;
}
function sameDelivery(a:Attached,b:Attached):boolean {
  return a.delivery.tabId===b.delivery.tabId&&a.delivery.generation===b.delivery.generation
    &&a.delivery.deliveryId===b.delivery.deliveryId&&a.delivery.deliveryEpoch===b.delivery.deliveryEpoch;
}
export function captureLayoutPresentation(current:ManagerAttachment|undefined,ready:ManagerAttachment|undefined):LayoutPresentation|undefined {
  if(!owned(current)||!owned(ready)||!sameControl(current,ready)||!sameDelivery(current,ready))return;
  const before=attachmentSchema.parse(current);if(!owned(before))return;
  return{source:current,before,identity:{tabId:before.input.tabId,generation:before.input.generation,
    controlEpoch:before.input.controlEpoch},valid:true};
}
export function validateLayoutPresentation(original:LayoutPresentation,value:ManagerAttachment,observe?:(outcome:LayoutComparisonOutcome)=>void):boolean {
  // The binding-captured original request authenticates the result. This merely
  // decides whether that result can keep the old UI desire, never its authority.
  const comparison=(matches:boolean,outcome:LayoutComparisonOutcome)=>{if(!matches)observe?.(outcome);return matches;};
  return comparison(original.valid,'intervening_retirement')
    &&owned(value,()=>observe?.('response_not_owned'))
    &&sameControl(original.before,value,()=>observe?.('response_control_changed'))
    &&comparison(value.delivery.tabId===original.before.delivery.tabId,'response_generation_changed')
    &&comparison(value.delivery.generation===original.before.delivery.generation,'response_generation_changed');
}
export function layoutNotice(value:LayoutPresentation,phase:NonNullable<NativeLayoutPresentation['phase']>):NativeLayoutPresentation {
  return{kind:'layoutRecovery',phase,identity:{...value.identity}};
}
