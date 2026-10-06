import { describe, expect, it } from 'vitest';
import { LinuxBackend } from './index';
import { hasSessionWindowBackend } from './session-windows';
import { settingsSchema, probeSchema, processIdentitySchema } from '../../shared/schemas';
import { defaultSettings } from '../defaults';

describe('Ubuntu contract and capability limits', () => {
  it('accepts real GNOME/Bash settings without Windows impersonation', () => {
    expect(settingsSchema.safeParse({ ...defaultSettings, adapterId: 'gnome-terminal', shellId: 'bash', shellExecutable: '/usr/bin/bash' }).success).toBe(true);
  });
  it('does not offer Windows-only window lifecycle/adoption APIs', () => {
    expect(hasSessionWindowBackend(new LinuxBackend())).toBe(false);
  });
  it('preserves exact boot identity and kernel start ticks as a bounded decimal string', () => {
    expect(processIdentitySchema.safeParse({ pid: 42, startTime: '123456789012345678901234567890123456789012345678901234567890' }).success).toBe(true);
    expect(processIdentitySchema.safeParse({ pid: 42, startTime: '1'.repeat(81) }).success).toBe(false);
  });
  it('advertises launch/tracking but no focus/reopen/append invention', () => {
    const probe = { platform: 'linux', arch: 'x64', adapterId: 'gnome-terminal', available: true, terminalVersion: null, capabilities: { createWindow: true, addTab: false, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: false, commandExitStatus: false, processTracking: true, explorerContextMenu: false }, shells: [{ id: 'bash', executable: '/usr/bin/bash', available: true, reason: null }], reasons: ['Only initial GNOME windows and scoped Bash descendants.'] };
    expect(probeSchema.safeParse(probe).success).toBe(true);
  });
  it('still refuses target-based append before contacting a Linux helper', async () => {
    const id='00000000-0000-4000-8000-000000000001';const target={kind:'windows-terminal' as const,windowName:`shellfox-${id}`,hwnd:'123',owner:{pid:1,startTime:'100'},sessionId:id,markerPrefix:`SHELLFOX:${id}:`,verification:'native-title' as const};
    const result=await new LinuxBackend().launch({sessionId:id,tabId:id,operationId:id,cwd:'/home/test',shellId:'bash',shellExecutable:'/usr/bin/bash',windowName:`shellfox-${id}`,titleMarker:`SHELLFOX:${id}:${id}`,existingTarget:target});
    expect(result).toMatchObject({ok:false,error:{code:'UNSUPPORTED'}});
  });
});
