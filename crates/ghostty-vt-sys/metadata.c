#include "adapter.h"
#include <stdlib.h>
#include <string.h>
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) return r; } while (0)

struct SfMetadata {
    uint64_t version;
    uint32_t bell_count, rejected_updates;
    bool pending, bells_saturated;
    size_t title_len, cwd_uri_len;
    uint8_t title[SF_METADATA_TITLE_MAX_BYTES];
    uint8_t cwd_uri[SF_METADATA_CWD_MAX_BYTES];
};

static void changed(SfVt *vt) {
    if (vt->metadata->version == UINT64_MAX) {
        /* Monotonic versions cannot wrap. The owner must stop on this error. */
        vt->callback_error = GHOSTTY_LIMIT_EXCEEDED;
        return;
    }
    vt->metadata->version++;
    vt->metadata->pending = true;
}

static void rejected(SfVt *vt) {
    if (vt->metadata->rejected_updates < SF_METADATA_COUNT_MAX) vt->metadata->rejected_updates++;
    changed(vt);
}

/* Public title/PWD callbacks explicitly permit these read-only getters.
   Only fixed C-owned buffers are touched; no Rust or parser mutation re-entry. */
static void title_changed(GhosttyTerminal terminal, void *userdata) {
    SfVt *vt = userdata;
    if (vt->callback_error) return;
    GhosttyString value = {0};
    uint8_t normalized[SF_METADATA_TITLE_MAX_BYTES];
    size_t len = 0;
    if (ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_TITLE, &value) ||
        !sf_metadata_title(value.ptr, value.len, normalized, &len)) {
        rejected(vt);
        return;
    }
    SfMetadata *metadata = vt->metadata;
    if (len == metadata->title_len && (!len || !memcmp(normalized, metadata->title, len))) return;
    if (len) memcpy(metadata->title, normalized, len);
    metadata->title_len = len;
    changed(vt);
}

static void pwd_changed(GhosttyTerminal terminal, void *userdata) {
    SfVt *vt = userdata;
    if (vt->callback_error) return;
    GhosttyString value = {0};
    if (ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_PWD, &value) ||
        !sf_metadata_cwd(value.ptr, value.len)) {
        rejected(vt);
        return;
    }
    SfMetadata *metadata = vt->metadata;
    if (value.len == metadata->cwd_uri_len && (!value.len || !memcmp(value.ptr, metadata->cwd_uri, value.len))) return;
    if (value.len) memcpy(metadata->cwd_uri, value.ptr, value.len);
    metadata->cwd_uri_len = value.len;
    changed(vt);
}

static void bell(GhosttyTerminal terminal, void *userdata) {
    (void)terminal;
    SfVt *vt = userdata;
    if (vt->callback_error) return;
    if (vt->metadata->bell_count < SF_METADATA_COUNT_MAX) vt->metadata->bell_count++;
    else vt->metadata->bells_saturated = true;
    changed(vt);
}

static void reset(GhosttyTerminal terminal, void *userdata) {
    (void)terminal;
    SfVt *vt = userdata;
    if (vt->callback_error) return;
    if (!vt->metadata->title_len && !vt->metadata->cwd_uri_len) return;
    vt->metadata->title_len = 0;
    vt->metadata->cwd_uri_len = 0;
    changed(vt);
}

int sf_metadata_register(SfVt *vt) {
    vt->metadata = calloc(1, sizeof(*vt->metadata));
    if (!vt->metadata) return GHOSTTY_OUT_OF_MEMORY;
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_TITLE_CHANGED, (const void *)title_changed));
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_PWD_CHANGED, (const void *)pwd_changed));
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_BELL, (const void *)bell));
    return ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_RESET, (const void *)reset);
}

void sf_metadata_free(SfVt *vt) { free(vt->metadata); }

int sf_metadata_peek(SfVt *vt, SfMetadataView *out) {
    if (vt->callback_error) return vt->callback_error;
    SfMetadata *metadata = vt->metadata;
    if (!metadata || !metadata->pending) return GHOSTTY_NO_VALUE;
    out->version = metadata->version;
    out->title = metadata->title;
    out->title_len = metadata->title_len;
    out->cwd_uri = metadata->cwd_uri;
    out->cwd_uri_len = metadata->cwd_uri_len;
    out->bell_count = metadata->bell_count;
    out->rejected_updates = metadata->rejected_updates;
    out->bells_saturated = metadata->bells_saturated;
    return 0;
}

int sf_metadata_ack(SfVt *vt, uint64_t version) {
    if (vt->callback_error) return vt->callback_error;
    if (!vt->metadata || !vt->metadata->pending || vt->metadata->version != version) return GHOSTTY_INVALID_VALUE;
    vt->metadata->pending = false;
    vt->metadata->bell_count = 0;
    vt->metadata->bells_saturated = false;
    vt->metadata->rejected_updates = 0;
    return 0;
}
