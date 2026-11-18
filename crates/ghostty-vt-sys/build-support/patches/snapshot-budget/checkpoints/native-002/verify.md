# Reproduce this producer candidate

Use a fresh source extraction under project `tmp/`. Verify the retained signed
archive pins first. Do not apply over another worker's tree or staged artifacts.
No Node, GUI, account or mobile runtime is needed by either codec or C consumer.

1. Extract source/deps from `build-support/vendor`. Use exact Zig 0.17.0 from
   project `tmp/toolchains`; its verified Windows archive pin is in checkpoint.json.
2. Apply the exact required Native002 patches 1, 2, then this patch 3 with
   `patch --batch --fuzz=0 -p1`. Check all three SHA-256 values first.
3. Compute SHA-256 of `engine-profile-input.txt`. Expected declared test identity
   is `939d2a42b92dab06a5d9879bf75ecfc44918efcb27ea40e678038e00f55f33b2`.
4. Set `ZIG_GLOBAL_CACHE_DIR` and all output/cache/prefix paths below project tmp.
   Supply these build flags explicitly, without relying on a parent Git worktree:

```text
-Dversion-string=1.4.1-dev -Dlib-version-string=1.0.1
-Doptimize=ReleaseSafe -Dcpu=baseline -Dtarget=x86_64-windows-gnu
-Dshellfox-engine-profile=939d2a42b92dab06a5d9879bf75ecfc44918efcb27ea40e678038e00f55f33b2
```

Run with a finite process timeout and `-j1`:

```text
zig build test-lib-vt --system DEPS --cache-dir CACHE FLAGS
  -Dtest-filter=snapshot -Dtest-filter=Shellfox -Dtest-shards=1 --summary all
zig build --system DEPS --cache-dir CACHE --prefix INSTALL FLAGS -Demit-lib-vt
```

The combined suite passed 452/453, with one intentional unavailable-build test
skip on this supported build. Run `-Dtest-filter='snapshot budget'` alone for the
92/93 scoped result. No whole-upstream-suite or all-platform execution is claimed.

Compile `tests/public-api.c` from the snapshot-budget directory against
INSTALL/include and INSTALL/lib/ghostty-vt.lib with the same local Zig C driver,
C11, `-Wall -Wextra -Werror`. Place the executable beside INSTALL/bin/ghostty-vt.dll
and invoke it with the declared test identity as its single argument. The consumer
uses only public headers and checks actual State/body, live suffix, configuration,
identity, metadata feature bits, failure cleanup and Legacy refusal.

Expected final line:

```text
public Native snapshot State/body ABI passed; decode=184 encode=168 State=24 report=48; resize offsets=148/132
```

These are new-header/library observations. Earlier transient layouts happened to
have the same sizes; that is not compatibility. Use header/patch/build digests.

The first combined test-plus-build outer command exhausted a 240-second timeout
after tests passed. The separate library rerun completed. Do not turn a timeout
into a pass claim; checkpoint.json records that history and the actual final DLL
and C-consumer hashes. `finalAcceptedEngineProfile` remains null.
