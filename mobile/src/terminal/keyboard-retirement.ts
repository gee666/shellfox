import {z} from 'zod';

export const keyboardClearReasonSchema=z.enum([
  'explicit_mode_close','selection_close','bridge_non_attached','native_state_not_retained',
  'native_readiness_invalid','native_error','session_error','layout_validation_lost',
  'ready_context_lost','request_context_invalid','visibility_hidden_after_ready',
  'view_effect_cleanup','unknown',
]);
export const layoutComparisonOutcomeSchema=z.enum([
  'before_context_unavailable','pending_unrelated','binding_retired','source_metadata_replaced',
  'intervening_retirement','response_not_owned','response_control_changed','response_generation_changed',
  'request_failed_or_unknown','validated',
]);
export type KeyboardClearReason=z.infer<typeof keyboardClearReasonSchema>;
export type LayoutComparisonOutcome=z.infer<typeof layoutComparisonOutcomeSchema>;
export const keyboardRetirementSchema=z.object({
  intent:z.enum(['show','hide']),reason:keyboardClearReasonSchema,comparison:layoutComparisonOutcomeSchema.optional(),
}).strict();
export type KeyboardRetirement=z.infer<typeof keyboardRetirementSchema>;
export function retirementRecord(intent:KeyboardRetirement['intent'],reason:unknown,comparison?:unknown):KeyboardRetirement {
  const parsed=keyboardClearReasonSchema.safeParse(reason),outcome=layoutComparisonOutcomeSchema.safeParse(comparison);
  return{intent,reason:parsed.success?parsed.data:'unknown',...(outcome.success?{comparison:outcome.data}:{})};
}
export function retirementNotice(value:KeyboardRetirement):string {
  return `Historical Keyboard ${value.intent==='show'?'SHOW':'HIDE'} ended: ${value.reason}`+
    (value.comparison?` / ${value.comparison}`:'')+'. Source branch only; not current focus or OS hide cause.';
}
