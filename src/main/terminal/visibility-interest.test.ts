import { describe, expect, it, vi } from 'vitest';
import { VisibilityInterest } from './visibility-interest';

describe('local and remote process polling interest', () => {
  it('unions independent views and removes only the disconnected client', () => {
    const publish = vi.fn(); const interest = new VisibilityInterest(publish);
    interest.setLocal(['local', 'shared']);
    interest.setRemote('laptop', ['remote', 'shared']);
    interest.setRemote('phone', ['phone']);
    expect(publish).toHaveBeenLastCalledWith(['local', 'phone', 'remote', 'shared']);
    interest.setLocal([]);
    expect(publish).toHaveBeenLastCalledWith(['phone', 'remote', 'shared']);
    interest.removeRemote('laptop');
    expect(publish).toHaveBeenLastCalledWith(['phone']);
    interest.removeRemote('phone');
    expect(publish).toHaveBeenLastCalledWith([]);
  });
  it('deduplicates union updates and copies input arrays', () => {
    const publish = vi.fn(); const interest = new VisibilityInterest(publish);
    const sessions = ['b', 'a'];
    interest.setLocal(sessions); sessions.push('not-visible');
    interest.setRemote('laptop', ['a']);
    interest.setRemote('laptop', ['a']);
    interest.removeRemote('unknown');
    expect(publish).toHaveBeenCalledTimes(1);
    expect(publish).toHaveBeenLastCalledWith(['a', 'b']);
  });
  it('bounds remote view count but permits updating and replacing existing views', () => {
    const interest = new VisibilityInterest(() => {});
    for (let id = 0; id < 32; id++) expect(interest.setRemote(String(id), [])).toBe(true);
    expect(interest.setRemote('extra', [])).toBe(false);
    expect(interest.setRemote('0', ['visible'])).toBe(true);
    interest.removeRemote('0');
    expect(interest.setRemote('extra', [])).toBe(true);
  });
});
