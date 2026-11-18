import { beforeEach, expect, it, vi } from 'vitest';

const bridge = vi.hoisted(() => ({
  start: vi.fn(), status: vi.fn(), peers: vi.fn(), hosts: vi.fn(), login: vi.fn(),
  request: vi.fn(), stop: vi.fn(), logout: vi.fn(),
}));
vi.mock('react-native', () => ({ NativeModules: { ShellfoxNetwork: bridge } }));
import { discoverHosts, discoverManagerCandidates, networkPeers } from './network';

const host = { id: 'n-host', hostname: 'plain-laptop', addresses: ['100.100.1.2'], online: true };
beforeEach(() => { vi.resetAllMocks(); });
it('reads only native verified hosts and preserves active discovery alongside cached hosts', async () => {
  bridge.peers.mockResolvedValue(JSON.stringify({ peers: [{ ...host, id: 'ordinary', hostname: 'shellfox-server' }] }));
  bridge.hosts.mockResolvedValue(JSON.stringify({ peers: [host], discovering: true }));
  expect(await discoverHosts()).toEqual({ peers: [host], discovering: true });
  expect(bridge.hosts).toHaveBeenCalledOnce(); expect(bridge.peers).not.toHaveBeenCalled();
});
it('uses the authenticated Go map only as explicit Manager candidates, without a legacy signature probe',async()=>{
  bridge.peers.mockResolvedValue(JSON.stringify({peers:[host,{...host,id:'offline',online:false}]}));
  expect(await discoverManagerCandidates()).toEqual({peers:[host,{...host,id:'offline',online:false}],discovering:false});
  expect(bridge.peers).toHaveBeenCalledOnce();expect(bridge.hosts).not.toHaveBeenCalled();expect(bridge.request).not.toHaveBeenCalled();
});
it.each(['{','{"peers":[],"hostConfirmed":true}',JSON.stringify({peers:Array.from({length:1001},()=>host)})])('rejects malformed or authority-added Manager candidate maps',async raw=>{
  bridge.peers.mockResolvedValue(raw);await expect(discoverManagerCandidates()).rejects.toThrow();expect(bridge.hosts).not.toHaveBeenCalled();
});
it('distinguishes an initial empty scan from an empty settled result', async () => {
  bridge.hosts.mockResolvedValueOnce('{"peers":[],"discovering":true}').mockResolvedValueOnce('{"peers":[],"discovering":false}');
  expect(await discoverHosts()).toEqual({ peers: [], discovering: true });
  expect(await discoverHosts()).toEqual({ peers: [], discovering: false });
});
it('keeps the peers-only wrapper restricted to confirmed hosts', async () => {
  bridge.hosts.mockResolvedValue(JSON.stringify({ peers: [{ ...host, online: false }], discovering: false }));
  expect(await networkPeers()).toEqual([{ ...host, online: false }]);
  expect(bridge.peers).not.toHaveBeenCalled();
});
it.each([
  '{', '{"peers":[]}', '{"peers":[],"discovering":"true"}',
  '{"peers":[],"discovering":false,"url":"http://localhost"}',
  '{"peers":[{"id":"n-host"}],"discovering":false}',
  JSON.stringify({ peers: Array.from({ length: 1001 }, () => host), discovering: true }),
])('rejects malformed or unbounded discovery responses', async raw => {
  bridge.hosts.mockResolvedValue(raw);
  await expect(discoverHosts()).rejects.toThrow();
});
it('propagates native failure without exposing ordinary peers as a fallback', async () => {
  bridge.hosts.mockRejectedValue(new Error('Network operation failed.'));
  await expect(discoverHosts()).rejects.toThrow('Network operation failed.');
  expect(bridge.peers).not.toHaveBeenCalled();
});
