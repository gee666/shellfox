import {z} from 'zod';
import {positiveDecimalSchema} from '../rust-client/identifiers';
const attempted=z.enum(['notAttempted','accepted','rejected']);
const observations={attached:z.boolean(),windowFocused:z.boolean(),uiThread:z.boolean(),inputAllowed:z.boolean(),
  frameAvailable:z.boolean(),focusRequest:attempted,focused:z.boolean()};
export const focusResultSchema=z.discriminatedUnion('platform',[
  z.object({platform:z.literal('android'),source:z.enum(['keyboard','surface','attachment']),
    ...observations,keyboardRequest:attempted}).strict(),
  z.object({platform:z.literal('ios'),source:z.enum(['keyboard','surface','accessibility','readiness']),
    ...observations,keyboardRequest:z.literal('notApplicable')}).strict(),
]);
export const focusFeedbackSchema=z.object({terminalHandle:positiveDecimalSchema,state:z.literal('focusResult'),
  focusResult:focusResultSchema,
  // React Native may supply this envelope field. Never retain or display it.
  target:z.number().int().min(0).max(Number.MAX_SAFE_INTEGER).optional(),
}).strict().transform(({target:_target,...feedback})=>feedback);
// RN codegen emits optional nested event objects as a value struct on iOS.
// Its untouched default is not feedback. Reject every other unexpected body.
const emptyGeneratedResult=z.object({platform:z.literal(''),source:z.literal(''),attached:z.literal(false),
  windowFocused:z.literal(false),uiThread:z.literal(false),inputAllowed:z.literal(false),frameAvailable:z.literal(false),
  focusRequest:z.literal(''),focused:z.literal(false),keyboardRequest:z.literal('')}).strict();
export function isFocusFeedback(event:{state:string;focusResult?:unknown}):boolean{
  return event.state==='focusResult'||event.focusResult!==undefined&&!emptyGeneratedResult.safeParse(event.focusResult).success;
}
export type FocusResult=z.infer<typeof focusResultSchema>;
export type FocusActionObservation=Readonly<{intent:'show'|'hide';snapshot:FocusResult|null}>;
export function focusNotice(value:FocusResult):string{
  // Historical action snapshot, never current focus or keyboard visibility.
  return `${value.platform}/${value.source}: focus=${value.focusRequest}, snapshot focused=${value.focused?'yes':'no'}, request=${value.keyboardRequest}; `+
    `attached=${value.attached?'yes':'no'}, window=${value.windowFocused?'yes':'no'}, UI=${value.uiThread?'yes':'no'}, input=${value.inputAllowed?'yes':'no'}, frame=${value.frameAvailable?'yes':'no'}.`;
}
export function focusActionNotice(value:FocusActionObservation):string{
  return `Keyboard ${value.intent==='show'?'SHOW':'HIDE'}. Snapshot since press: `+
    (value.snapshot?`observed; ${focusNotice(value.snapshot)}`:'notObserved.');
}
