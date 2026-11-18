/* Shellfox additive terminal-profile ABI. Native upstream APIs are unchanged. */
#ifndef GHOSTTY_VT_SHELLFOX_TERMINAL_PROFILE_H
#define GHOSTTY_VT_SHELLFOX_TERMINAL_PROFILE_H
#include <ghostty/vt/terminal.h>
#ifdef __cplusplus
extern "C" {
#endif
#define GHOSTTY_SHELLFOX_TERMINAL_PROFILE_ABI 1u
#define GHOSTTY_SHELLFOX_TERMINAL_PROFILE_NATIVE 0u
#define GHOSTTY_SHELLFOX_TERMINAL_PROFILE_LEGACY_XTERM6 1u

typedef struct {
  size_t size;
  uint32_t abi_version;
  GhosttyString engine_profile;
  uint32_t terminal_profile;
  uint16_t cols;
  uint16_t rows;
} GhosttyShellfoxTerminalProfileOptions;

/* One canonical State ABI. No serialized padding or decomposed width/join struct.
 * Native requires preceding_join=0. Legacy accepts exactly 0,2,3,4,5, the raw
 * UnicodeV6 property values. Capture at the exact consumed input cut; pending
 * continuation is separate. It must be installed BEFORE continuation replay.
 * This state says nothing about a process, UI, artifact authenticity, or cells.
 */
typedef struct {
  size_t size;
  uint32_t abi_version;
  uint32_t terminal_profile;
  uint32_t preceding_join;
} GhosttyShellfoxTerminalProfileState;

GHOSTTY_API GhosttyResult ghostty_shellfox_terminal_new_profile(
    const GhosttyAllocator* allocator, GhosttyTerminal* terminal,
    const GhosttyShellfoxTerminalProfileOptions* options);
GHOSTTY_API GhosttyResult ghostty_shellfox_terminal_get_profile(
    GhosttyTerminal terminal, uint32_t* terminal_profile);
/* Output storage must be this exact ABI/size. Caller owns terminal exclusively.
 * This getter is NOT an atomic snapshot encoder. Use Brooklyn's profile-aware
 * State/body companion, never a racy getter plus an unprofiled snapshot encoder.
 * Returns UNSUPPORTED if profile state is unavailable/incompatible.
 */
GHOSTTY_API GhosttyResult ghostty_shellfox_terminal_get_profile_state(
    GhosttyTerminal terminal, GhosttyShellfoxTerminalProfileState* state);
/* No public setter. Existing terminals cannot change parser profile/state. */
#ifdef __cplusplus
}
#endif
#endif
