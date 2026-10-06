import { it, expect } from 'vitest';
import { EarlyRequestQueue, parseCli, productArguments, validateForwarded } from './cli';
const id = 'c36951c8-03f5-49c7-bb11-ad8828044a31';
it.each([{ cwd: 'C:\\project space', args: ['start'], expected: 'C:\\project space' }, { cwd: 'C:\\project space', args: ['start', '..\\next'], expected: 'C:\\next' }, { cwd: '/tmp/project', args: ['start', '.'], expected: '/tmp/project' }])('resolves shellfox start relative to caller cwd before forwarding $cwd', ({ cwd, args, expected }) => { expect(parseCli(args, { cwd })).toMatchObject({ ok: true, value: { request: { kind: 'new-session', cwd: expected } } }); });
it('rejects combined start/legacy arguments and surplus paths', () => { for (const args of [['start', '.', 'extra'], ['start', '.', '--new-session'], ['--new-session', '--cwd', 'C:\\work', 'start', '.']]) expect(parseCli(args).ok).toBe(false); });
it('shows manager without product flags', () => expect(parseCli([])).toEqual({ok:true,value:{request:{version:1,kind:'show'}}}));
it.each([['--cwd','C:\\work'],['--new-session'],['--new-session','--cwd','relative'],['--new-session','--cwd',''],['--new-session','--new-session','--cwd','C:\\work'],['--run','bad'],['--new-session','--cwd','C:\\work','--cwd','C:\\other']].map(args => ({ args })))('rejects invalid CLI $args', ({ args }) => expect(parseCli(args).ok).toBe(false));
it('assigns different request IDs to same-folder invocations', () => {
  const a = parseCli(['--new-session','--cwd','C:\\work']); const b = parseCli(['--new-session','--cwd','C:\\work']);
  expect(a.ok && b.ok && a.value.request.kind==='new-session' && b.value.request.kind==='new-session' && a.value.request.requestId!==b.value.request.requestId).toBe(true);
});
it('separates known runtime switches without swallowing unknown product flags', () => {
  expect(productArguments(['--inspect=0','--remote-debugging-port=0','--new-session','--cwd','C:\\work'])).toEqual(['--new-session','--cwd','C:\\work']);
  expect(productArguments(['--run=evil'])).toEqual(['--run=evil']);
});
it('rejects forwarded extras', () => expect(validateForwarded({version:1,kind:'show',cwd:'C:\\work'}).ok).toBe(false));
it('gates test flags and enforces isolated test paths', () => {
  const root = process.cwd(); const tmp = root + '/tmp/tests/cli';
  expect(parseCli(['--test-user-data',tmp,'--test-backend','fake']).ok).toBe(false);
  expect(parseCli(['--test-user-data',tmp,'--test-backend','fake'],{testMode:true,testBuild:false,projectRoot:root}).ok).toBe(false);
  expect(parseCli(['--test-user-data',root,'--test-backend','real'],{testMode:true,testBuild:true,projectRoot:root}).ok).toBe(false);
  expect(parseCli(['--test-user-data',tmp,'--test-backend','fake'],{testMode:true,testBuild:true,projectRoot:root}).ok).toBe(true);
});
it('queues before ready, dedupes only request identity, and preserves same-folder deliveries', async () => {
  const queue = new EarlyRequestQueue(); const seen: string[] = [];
  const request = {version:1,kind:'new-session',requestId:id,cwd:'C:\\work'};
  queue.enqueue(request); queue.enqueue(request);
  queue.enqueue({...request,requestId:'c36951c8-03f5-49c7-bb11-ad8828044a32'});
  expect(seen).toEqual([]);
  queue.ready(async r => { if (r.kind === 'new-session') seen.push(r.requestId); });
  await queue.idle();
  expect(seen).toEqual([id,'c36951c8-03f5-49c7-bb11-ad8828044a32']);
});
