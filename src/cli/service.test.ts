import { expect, it } from 'vitest';
import { runHostService } from './service';

it('never hides confirmation in JSON or silently manages a remote service', async () => {
  await expect(runHostService(['uninstall'], process.cwd(), { json: true, yes: false })).rejects.toThrow('--yes');
  await expect(runHostService(['install'], process.cwd(), { json: true, yes: true, host: 'peer' })).rejects.toThrow('local host');
});
