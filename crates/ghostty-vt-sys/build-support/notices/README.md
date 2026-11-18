# Notice provenance

`LICENSE` is Ghostty's MIT license. `third-party.json` is the unmodified
`deps/libghostty-vt-third-party.json` from the pinned 1.0.1 source archive.
The simdutf licenses come from that archive's `pkg/simdutf/vendor/` directory.
Other dependency license files come from the corresponding package directories
in the pinned 1.0.1 dependency archive.

That dependency archive omits the `licenses/` directory referenced by uucode's
`LICENSE.md`. The following supplemental files were fetched from uucode's exact
commit recorded in `third-party.json`, without modifying their contents:

- `uucode/LICENSE_unicode`
- `uucode/LICENSE_Bjoern_Hoehrmann`

Their upstream URL prefix is
`https://raw.githubusercontent.com/jacobsandlund/uucode/1fb73433bba5d93366c57f23ff2e9d7939746500/licenses/`.
These supplemental files were acquired over HTTPS, not authenticated by
Ghostty's archive signature. Keep them with the uucode MIT license when
redistributing linked code.
