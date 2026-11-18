/* Opt-in Native002 bridge. Only compiled with the verified private extension. */
#include "adapter.h"
#ifdef SHELLFOX_EXPERIMENTAL_NATIVE
#include <ghostty/vt/shellfox_terminal_profile.h>
#include <stdlib.h>

int sf_experimental_native_borrow(SfVt *vt, GhosttyTerminal *out) {
    if (!out) return GHOSTTY_INVALID_VALUE;
    *out = NULL;
    if (!vt || !vt->terminal) return GHOSTTY_INVALID_VALUE;
    int result = sf_status(vt);
    if (result) return result;
    bool failed = false;
    result = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR, &failed);
    if (result) return result;
    if (failed) return GHOSTTY_IO_ERROR;
    *out = vt->terminal;
    return GHOSTTY_SUCCESS;
}

/* Only newly owned profile-constructor/budget-decoder handles may enter here.
   No callbacks are installed or rebound. The returned adapter is passive.
   Success consumes *owned; failure leaves it unchanged and clears *out. */
int sf_experimental_native_adopt(GhosttyTerminal *owned, SfVt **out) {
    if (!out) return GHOSTTY_INVALID_VALUE;
    *out = NULL;
    if (!owned || !*owned) return GHOSTTY_INVALID_VALUE;
    GhosttyShellfoxTerminalProfileState state = {0};
    state.size = sizeof(state); state.abi_version = 1;
    int result = ghostty_shellfox_terminal_get_profile_state(*owned, &state);
    if (result) return result;
    if (state.size != sizeof(state) || state.abi_version != 1 || state.terminal_profile || state.preceding_join) return GHOSTTY_UNSUPPORTED;
    uint16_t cols = 0, rows = 0;
    result = ghostty_terminal_get(*owned, GHOSTTY_TERMINAL_DATA_COLS, &cols);
    if (!result) result = ghostty_terminal_get(*owned, GHOSTTY_TERMINAL_DATA_ROWS, &rows);
    if (result) return result;
    if (!cols || !rows || (uint64_t)cols * rows > 1048576u) return GHOSTTY_INVALID_VALUE;
    bool failed = false;
    result = ghostty_terminal_get(*owned, GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR, &failed);
    if (result) return result;
    if (failed) return GHOSTTY_INVALID_VALUE;
    SfVt *vt = calloc(1, sizeof(*vt));
    if (!vt) return GHOSTTY_OUT_OF_MEMORY;
    result = sf_adapter_storage_new(vt);
    if (result) { sf_free(vt); return result; }
    /* No fallible operation or caller callback follows the ownership commit. */
    vt->cols = cols; vt->height = rows; vt->terminal = *owned;
    *owned = NULL; *out = vt;
    return GHOSTTY_SUCCESS;
}
#endif
