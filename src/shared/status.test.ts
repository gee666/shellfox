import { describe, it, expect } from 'vitest';
import { aggregateStatus, compareSessions, countTabs, publicStatus, tabStatus } from './status';
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
  it('counts live activity and failed closed tabs separately', () => expect(countTabs([tab('running','open',2),tab('unknown','closed'),tab('error','closed')])).toEqual({running:1,waiting:0,unknown:0,error:1,closed:2,agents:2}));
  it('requires a live observed root for running', () => {
    const o = { sessionId:'s',tabId:'t',observedAt:'time',root:'alive' as const,health:'unknown' as const,agents:1,reason:null };
    expect(tabStatus('open',null,o)).toBe('running');
    expect(tabStatus('open',null,{...o,root:'unavailable'})).toBe('unknown');
    expect(tabStatus('launching',null,o)).toBe('unknown');
    expect(tabStatus('open',error,o)).toBe('error');
  });
  it('keeps blue sessions in the same order when opening an ordinary shell', () => {
    const empty = { id: 'a', createdAt: '2026-01-01', status: 'unknown' as const };
    const other = { id: 'b', createdAt: '2026-01-02', status: 'waiting' as const };
    expect(compareSessions(empty, other)).toBeLessThan(0);
    expect(compareSessions({ ...empty, status: 'waiting' }, other)).toBeLessThan(0);
  });
  it('sorts visible status then creation time then ID', () => {
    const records = [
      {status:'unknown' as const,createdAt:'1',id:'a'}, {status:'waiting' as const,createdAt:'2',id:'b'},
      {status:'error' as const,createdAt:'1',id:'a'}, {status:'waiting' as const,createdAt:'2',id:'a'},
      {status:'running' as const,createdAt:'1',id:'a'}, {status:'settled' as const,createdAt:'1',id:'a'},
    ];
    expect(records.sort(compareSessions).map(r => r.status+':'+r.id)).toEqual(['running:a','error:a','unknown:a','waiting:a','waiting:b','settled:a']);
  });
});

describe('pinned session order', () => {
  it('puts pinned sessions first in pin order, then status/creation order', () => {
    const base = { createdAt: '2026-01-01T00:00:00.000Z' };
    const records = [
      { ...base, id: 'a', status: 'waiting' as const, pinnedAt: null },
      { ...base, id: 'b', status: 'running' as const, pinnedAt: '2026-02-02T00:00:00.000Z' },
      { ...base, id: 'c', status: 'unknown' as const, pinnedAt: '2026-02-01T00:00:00.000Z' },
      { ...base, id: 'd', status: 'waiting' as const },
    ];
    expect(records.sort(compareSessions).map(r => r.id)).toEqual(['c', 'b', 'a', 'd']);
  });
});
