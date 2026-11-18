import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { remoteMethods, remoteMethodSchema, remotePayloadSchemas, responseSchemas } from '../../src/shared/remote-schemas';

const fixtureSchema = z.array(z.object({ method: remoteMethodSchema, payload: z.unknown(), response: z.unknown() }).strict());
const cases = fixtureSchema.parse(JSON.parse(readFileSync(new URL('../../crates/shellfox-client/tests/fixtures/remote-parity.json', import.meta.url), 'utf8')));
describe('shared Rust client remaining remote contracts', () => {
  it('covers the complete active remote allowlist without manager privileges', () => {
    const methods = z.array(z.string()).parse(JSON.parse(readFileSync(new URL('../../crates/shellfox-client/tests/fixtures/remote-methods.json', import.meta.url), 'utf8')));
    expect([...methods].sort()).toEqual([...remoteMethods].sort());
  });
  for (const fixture of cases) {
    it(String(fixture.method), () => {
      const method = remoteMethodSchema.parse(fixture.method);
      expect(remotePayloadSchemas[method].parse(fixture.payload)).toEqual(fixture.payload);
      expect(responseSchemas[method].parse(fixture.response)).toEqual(fixture.response);
    });
  }
});
