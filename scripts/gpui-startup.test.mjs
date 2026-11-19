import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

// Source ordering only. Shared entry authentication/containment have their own owned fixtures.
test('shared early entry dispatch precedes normal parsing, ownership, logging and GPUI', () => {
  const source = readFileSync(new URL('../crates/shellfox-desktop/src/main.rs', import.meta.url), 'utf8');
  const main = source.slice(source.indexOf('fn main()'));
  const entry = main.indexOf('shellfox_service::dispatch_supervisor_entry()?');
  const args = main.indexOf('Options::parse(');
  const startup = main.indexOf('app_startup::prepare(&options)');
  const logging = main.indexOf('eprintln!(');
  const gui = main.indexOf('Application::new()');
  assert.ok(entry >= 0 && args > entry && startup > args && logging > startup && gui > startup);
  assert.match(main.slice(entry, args), /std::process::exit\(i32::from_ne_bytes\(code.to_ne_bytes\(\)\)\)/);
  assert.equal(main.split('dispatch_supervisor_entry(').length - 1, 1);
  assert.ok(!main.includes('shellfox_terminal::supervisor_entry()'));
  assert.ok(!main.includes('shellfox_terminal::raw_process::supervisor_entry()'));
});
