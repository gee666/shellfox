import {describe,expect,it} from 'vitest';
import {keyboardClearReasonSchema,layoutComparisonOutcomeSchema,keyboardRetirementSchema,retirementNotice,retirementRecord} from './keyboard-retirement';

describe('bounded action-local retirement metadata',()=>{
  it.each(keyboardClearReasonSchema.options)('permits only the closed reason %s',reason=>{
    expect(keyboardRetirementSchema.parse(retirementRecord('show',reason))).toEqual({intent:'show',reason});
  });
  it.each(layoutComparisonOutcomeSchema.options)('permits only the closed comparison %s',comparison=>{
    expect(retirementRecord('hide','layout_validation_lost',comparison)).toEqual({intent:'hide',reason:'layout_validation_lost',comparison});
  });
  it('drops malformed reasons and outcomes instead of displaying their payload',()=>{
    const value=retirementRecord('show',{token:'DO_NOT_DISPLAY'},'https://DO_NOT_DISPLAY');
    expect(value).toEqual({intent:'show',reason:'unknown'});expect(retirementNotice(value)).not.toContain('DO_NOT_DISPLAY');
    expect(keyboardRetirementSchema.safeParse({...value,epoch:1}).success).toBe(false);
    expect(keyboardRetirementSchema.safeParse({...value,reason:'controlId:secret'}).success).toBe(false);
  });
  it('labels a historical branch without asserting current focus, readiness or OS causality',()=>{
    const text=retirementNotice(retirementRecord('show','visibility_hidden_after_ready'));
    expect(text).toBe('Historical Keyboard SHOW ended: visibility_hidden_after_ready. Source branch only; not current focus or OS hide cause.');
  });
});
