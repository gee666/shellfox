#ifndef SHELLFOX_METADATA_H
#define SHELLFOX_METADATA_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define SF_METADATA_TITLE_MAX_BYTES 600u
#define SF_METADATA_TITLE_RAW_LIMIT 4096u
#define SF_METADATA_CWD_MAX_BYTES 32767u
#define SF_METADATA_COUNT_MAX 65535u

typedef struct SfMetadata SfMetadata;
typedef struct {
    uint64_t version;
    const uint8_t *title;
    size_t title_len;
    const uint8_t *cwd_uri;
    size_t cwd_uri_len;
    uint32_t bell_count, rejected_updates;
    uint8_t bells_saturated;
} SfMetadataView;

bool sf_metadata_title(const uint8_t *input, size_t len, uint8_t *out, size_t *out_len);
bool sf_metadata_cwd(const uint8_t *input, size_t len);
#endif
