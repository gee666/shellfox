// Fixed transport-only fixtures from src/host/automation.ts. No process data.
import { StringDecoder } from 'node:string_decoder';
const norm = text => text.replace(/\r?\n/g, '\r\n');
function render(events) {
  const d = [new StringDecoder('utf8'), new StringDecoder('utf8')];
  const writes = events.map(([stream, bytes]) => norm(d[stream].write(Buffer.from(bytes))));
  const end = norm(d[0].end() + d[1].end());
  return { writes, end, combined: writes.join('') + end };
}
const cases = {
  split_cross_stream: [[0,[0xf0,0x9f]],[1,[0x91,0xa9]]],
  complete_per_stream: [[0,[0xf0,0x9f]],[1,[0xe2]],[0,[0x91,0xa9]],[1,[0x82,0xac]]],
  eof_stdout_then_stderr: [[0,[0xf0,0x9f]],[1,[0xe2,0x82]]],
  cr_lf_interleaved: [[0,[97,13]],[1,[10,98,10]],[0,[10,99,13,10]]],
  cr_lf_same_stream_split: [[0,[108,13]],[0,[10,114]]],
  invalid_surrogate_split: [[0,[0xed,0xa0]],[1,[83]],[0,[0x80]]],
  invalid_overlong_split: [[0,[0xc0]],[1,[83]],[0,[0x80]]],
};
for (const [name, events] of Object.entries(cases)) console.log(JSON.stringify({ name, ...render(events) }));
let hash = 0xcbf29ce484222325n;
function add(text) {
  const bytes = Buffer.from(text);
  for (const byte of [bytes.length & 255, (bytes.length >>> 8) & 255, ...bytes]) {
    hash ^= BigInt(byte); hash = BigInt.asUintN(64, hash * 0x100000001b3n);
  }
}
for (let a=0;a<256;a++) for(let b=0;b<256;b++) {
  const value = render([[0,[a]],[1,[83]],[0,[b]]]);
  for(const write of value.writes) add(write);
  add(value.end);
}
console.log(JSON.stringify({ all_byte_pairs_interleaved_hash: hash.toString(16) }));
