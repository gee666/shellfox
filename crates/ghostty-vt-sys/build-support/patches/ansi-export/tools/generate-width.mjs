// Port only the pinned shipped xterm UnicodeV6 data, never an arbitrary TS program.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
const [source, destination] = process.argv.slice(2);
if (!source || !destination) throw new Error('usage: node generate-width.mjs UnicodeV6.ts output.zig');
const bytes = readFileSync(source);
const digest = '9ebb3df126129de4250ef5c1730493b7b3f62b3593273dac0bbbe15ee0cc890b';
if (createHash('sha256').update(bytes).digest('hex') !== digest) throw new Error('Unapproved UnicodeV6 source');
const input = bytes.toString('utf8');
function intervals(name) {
  const body = new RegExp(`const ${name} = \\[([\\s\\S]*?)\\];`).exec(input)?.[1];
  if (!body) throw new Error(`Missing ${name}`);
  const entries = [...body.matchAll(/\[(0x[0-9a-f]+),\s*(0x[0-9a-f]+)\]/gi)].map(match => [Number(match[1]), Number(match[2])]);
  if (!entries.length) throw new Error(`Empty ${name}`);
  entries.forEach(([a,b],index) => { if (a>b || b>0x10ffff || index && entries[index-1][1]>=a) throw new Error('Invalid range order'); });
  return `const ${name.toLowerCase()} = [_][2]u21{\n${entries.map(([a,b])=>`    .{ 0x${a.toString(16)}, 0x${b.toString(16)} },`).join('\n')}\n};\n`;
}
const header = `// Copyright (c) 2019 The xterm.js authors. All rights reserved. MIT.\n// Ported from @xterm/xterm 6.0.0 UnicodeV6.ts, SHA256 ${digest}.\n// See the retained xterm LICENSE and provenance next to this patch.\npub const profile = "xterm-6.0.0-unicode6";\n`;
const implementation = `
fn contains(cp:u21, ranges:[]const [2]u21) bool {
    var low:usize=0; var high:usize=ranges.len;
    while(low<high) { const mid=low+(high-low)/2;
        if(cp<ranges[mid][0]) high=mid else if(cp>ranges[mid][1]) low=mid+1 else return true;
    } return false;
}
pub fn width(cp:u21) u2 {
    if(cp<32) return 0;
    if(cp<127) return 1;
    if(cp<65536) {
        if(cp<0xa0 or contains(cp,&bmp_combining)) return 0;
        if((cp>=0x1100 and cp<0x1160) or cp==0x2329 or cp==0x232a or
          (cp>=0x2e80 and cp<0xa4d0 and cp!=0x303f) or
          (cp>=0xac00 and cp<0xd7a4) or (cp>=0xf900 and cp<0xfb00) or
          (cp>=0xfe10 and cp<0xfe1a) or (cp>=0xfe30 and cp<0xfe70) or
          (cp>=0xff00 and cp<0xff61) or (cp>=0xffe0 and cp<0xffe7)) return 2;
        return 1;
    }
    if(contains(cp,&high_combining)) return 0;
    if((cp>=0x20000 and cp<=0x2fffd) or (cp>=0x30000 and cp<=0x3fffd)) return 2;
    return 1;
}
pub fn validState(preceding:u32) bool {
    return switch(preceding) {0,2,3,4,5=>true,else=>false};
}
pub fn properties(cp:u21, preceding:u32) ?u32 {
    if(!validState(preceding)) return null;
    var current:u32=width(cp);
    const old=(preceding>>1)&3;
    const join=current==0 and old!=0;
    if(join) current=old;
    return (current<<1)|@intFromBool(join);
}
// UnicodeV6.charProperties joins only width-zero codepoints to a nonzero
// preceding cell; it does not join emoji/RI sequences into modern graphemes.
pub fn clusterWidth(base:u21, extra:[]const u21) ?u2 {
    const result=width(base); if(result==0) return null;
    for(extra) |cp| if(width(cp)!=0) return null;
    return result;
}
`;
writeFileSync(destination, header + intervals('BMP_COMBINING') + intervals('HIGH_COMBINING') + implementation);
