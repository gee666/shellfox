import { describe, it, expect } from 'vitest';
import { aggregateStatus, compareSessions, countTabs, needsSettleConfirmation, publicStatus, tabStatus } from './status';
import type { TabDto } from './contracts';
const tab = (status: TabDto['status'], lifecycle: TabDto['lifecycle'] = 'open', agents = 0) => ({status, lifecycle, agents});
const error = { code: 'LAUNCH_FAILED' as const, message: 'Failed', retryable: true };
describe('status rules', () => {
  it.each([
    [[tab('waiting'),tab('unknown'),tab('running'),tab('error')], 'error'],
    [[tab('waiting'),tab('unknown'),tab('running')], 'running'],
    [[tab('waiting'),tab('unknown')], 'unknown'],
    [[tab('waiting')], 'waiting'],
    [[tab('waiting','closed')], 'unknown'],
    [[tab('error','closed')], 'error'],
    [[], 'unknown'],
  ] as const)('aggregates %j to %s', (tabs, expected) => expect(aggregateStatus([...tabs])).toBe(expected));
  it('sticky session failures dominate', () => expect(aggregateStatus([tab('running')], error)).toBe('error'));
  it('settling overrides only public status', () => { expect(publicStatus('running', 'date')).toBe('settled'); expect(publicStatus('running', null)).toBe('running'); });
  it.each(['running','unknown'] as const)('confirms %s', status => expect(needsSettleConfirmation([tab(status)])).toBe(true));
  it('active agents and uncertain lifecycles require confirmation regardless of error badge', () => {
    expect(needsSettleConfirmation([tab('error','open',2)])).toBe(true);
    expect(needsSettleConfirmation([tab('error','launching')])).toBe(true);
    expect(needsSettleConfirmation([tab('error','launch-uncertain')])).toBe(true);
    expect(needsSettleConfirmation([tab('waiting','open',0)])).toBe(false);
  });
  it('closed unknown is not active', () => expect(needsSettleConfirmation([tab('unknown','closed')])).toBe(false));
  it('counts live activity and failed closed tabs separately', () => expect(countTabs([tab('running','open',2),tab('unknown','closed'),tab('error','closed')])).toEqual({running:1,waiting:0,unknown:0,error:1,closed:2,agents:2}));
  it('requires a live observed root for running', () => {
    const o = { sessionId:'s',tabId:'t',observedAt:'time',root:'alive' as const,health:'unknown' as const,agents:1,reason:null };
    expect(tabStatus('open',null,o)).toBe('running');
    expect(tabStatus('open',null,{...o,root:'unavailable'})).toBe('unknown');
    expect(tabStatus('launching',null,o)).toBe('unknown');
    expect(tabStatus('open',error,o)).toBe('error');
  });
  it('sorts status then creation time then ID', () => {
    const records = [
      {status:'unknown' as const,createdAt:'1',id:'a'}, {status:'waiting' as const,createdAt:'2',id:'b'},
      {status:'error' as const,createdAt:'1',id:'a'}, {status:'waiting' as const,createdAt:'2',id:'a'},
      {status:'running' as const,createdAt:'1',id:'a'}, {status:'settled' as const,createdAt:'1',id:'a'},
    ];
    expect(records.sort(compareSessions).map(r => r.status+':'+r.id)).toEqual(['waiting:a','waiting:b','error:a','running:a','unknown:a','settled:a']);
  });
});
