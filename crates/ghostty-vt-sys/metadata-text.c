#include "metadata.h"
#include <string.h>

typedef struct {
    const uint8_t *ptr;
    size_t len, index;
    bool percent;
} Reader;

static int hex(uint8_t byte) {
    if (byte >= '0' && byte <= '9') return byte - '0';
    if (byte >= 'a' && byte <= 'f') return byte - 'a' + 10;
    if (byte >= 'A' && byte <= 'F') return byte - 'A' + 10;
    return -1;
}

static int next_byte(Reader *reader) {
    if (reader->index == reader->len) return -1;
    int byte = reader->ptr[reader->index++];
    if (byte != '%' || !reader->percent) return byte;
    if (reader->len - reader->index < 2) return -1;
    int high = hex(reader->ptr[reader->index++]);
    int low = hex(reader->ptr[reader->index++]);
    return high < 0 || low < 0 ? -1 : high * 16 + low;
}

/* Strict scalar decoding rejects overlong forms, surrogates and truncation.
   Percent mode validates the decoded URI without retaining a decoded path. */
static int next_scalar(Reader *reader, uint32_t *out) {
    if (reader->index == reader->len) return 0;
    int first = next_byte(reader);
    if (first < 0) return -1;
    uint32_t scalar;
    unsigned continuation;
    uint32_t minimum;
    if (first < 0x80) { *out = (uint32_t)first; return 1; }
    if (first >= 0xc2 && first <= 0xdf) {
        scalar = (uint32_t)first & 0x1f; continuation = 1; minimum = 0x80;
    } else if (first >= 0xe0 && first <= 0xef) {
        scalar = (uint32_t)first & 0x0f; continuation = 2; minimum = 0x800;
    } else if (first >= 0xf0 && first <= 0xf4) {
        scalar = (uint32_t)first & 0x07; continuation = 3; minimum = 0x10000;
    } else return -1;
    for (unsigned i = 0; i < continuation; i++) {
        int byte = next_byte(reader);
        if (byte < 0x80 || byte > 0xbf) return -1;
        scalar = (scalar << 6) | ((uint32_t)byte & 0x3f);
    }
    if (scalar < minimum || scalar > 0x10ffff || (scalar >= 0xd800 && scalar <= 0xdfff)) return -1;
    *out = scalar;
    return 1;
}

static bool control(uint32_t scalar) {
    return scalar < 0x20 || (scalar >= 0x7f && scalar <= 0x9f);
}

/* Matches the Unicode whitespace trimmed by the existing title presentation. */
static bool space(uint32_t scalar) {
    return scalar == 0x20 || scalar == 0xa0 || scalar == 0x1680 ||
        (scalar >= 0x2000 && scalar <= 0x200a) || scalar == 0x2028 ||
        scalar == 0x2029 || scalar == 0x202f || scalar == 0x205f ||
        scalar == 0x3000 || scalar == 0xfeff;
}

bool sf_metadata_title(const uint8_t *input, size_t len, uint8_t *out, size_t *out_len) {
    if (len > SF_METADATA_TITLE_RAW_LIMIT || (len && !input)) return false;
    Reader reader = {input, len, 0, false};
    uint32_t scalar;
    int result;
    while ((result = next_scalar(&reader, &scalar)) > 0) {}
    if (result < 0) return false;
    reader.index = 0;
    size_t written = 0, trimmed = 0;
    unsigned units = 0;
    while (reader.index < len) {
        size_t start = reader.index;
        if (next_scalar(&reader, &scalar) != 1) return false;
        if (control(scalar) || (!written && space(scalar))) continue;
        unsigned width = scalar > 0xffff ? 2 : 1;
        if (units + width > 200) break;
        size_t bytes = reader.index - start;
        if (bytes > SF_METADATA_TITLE_MAX_BYTES - written) return false;
        memcpy(out + written, input + start, bytes);
        written += bytes;
        units += width;
        if (!space(scalar)) trimmed = written;
    }
    *out_len = trimmed;
    return true;
}

static bool scheme(const uint8_t *input) {
    const char *expected = "file://";
    for (size_t i = 0; i < 7; i++) {
        uint8_t byte = input[i];
        if (byte >= 'A' && byte <= 'Z') byte = (uint8_t)(byte + 'a' - 'A');
        if (byte != (uint8_t)expected[i]) return false;
    }
    return true;
}

static bool ipv4(const uint8_t *input, size_t len) {
    unsigned parts = 0;
    size_t index = 0;
    while (index < len) {
        unsigned value = 0, digits = 0;
        while (index < len && input[index] != '.') {
            if (input[index] < '0' || input[index] > '9' || ++digits > 3) return false;
            value = value * 10 + input[index++] - '0';
        }
        if (!digits || value > 255 || ++parts > 4) return false;
        if (index < len && ++index == len) return false;
    }
    return parts == 4;
}

static bool ipv6(const uint8_t *input, size_t len) {
    if (!len) return false;
    size_t index = 0;
    unsigned groups = 0;
    bool compressed = false;
    if (input[0] == ':') {
        if (len < 2 || input[1] != ':') return false;
        compressed = true;
        index = 2;
    }
    while (index < len) {
        size_t start = index;
        bool dotted = false;
        while (index < len && input[index] != ':') {
            if (input[index] == '.') dotted = true;
            index++;
        }
        if (dotted) {
            if (index != len || !ipv4(input + start, index - start)) return false;
            groups += 2;
            break;
        }
        size_t digits = index - start;
        if (!digits || digits > 4) return false;
        for (size_t i = start; i < index; i++) if (hex(input[i]) < 0) return false;
        if (++groups > 8) return false;
        if (index == len) break;
        if (++index == len) return false;
        if (input[index] == ':') {
            if (compressed) return false;
            compressed = true;
            index++;
        }
    }
    return compressed ? groups < 8 : groups == 8;
}

static bool authority(const uint8_t *input, size_t len) {
    if (len >= 2 && input[0] == '[' && input[len-1] == ']') return ipv6(input + 1, len - 2);
    Reader reader = {input, len, 0, true};
    uint32_t scalar;
    int result;
    while ((result = next_scalar(&reader, &scalar)) > 0) {
        if (control(scalar) || space(scalar)) return false;
        if (scalar >= 0x80) continue;
        if (!((scalar >= 'a' && scalar <= 'z') || (scalar >= 'A' && scalar <= 'Z') ||
              (scalar >= '0' && scalar <= '9') || scalar == '.' || scalar == '-' ||
              scalar == '_' || scalar == '~' || scalar == '!' || scalar == '$' ||
              scalar == '&' || scalar == '\'' || scalar == '(' || scalar == ')' ||
              scalar == '*' || scalar == '+' || scalar == ',' || scalar == ';' || scalar == '=')) return false;
    }
    return result == 0;
}

bool sf_metadata_cwd(const uint8_t *input, size_t len) {
    if (len == 0) return true;
    if (!input || len < 8 || len > SF_METADATA_CWD_MAX_BYTES || !scheme(input)) return false;
    size_t slash = 7;
    while (slash < len && input[slash] != '/') slash++;
    if (slash == len || !authority(input + 7, slash - 7)) return false;
    Reader reader = {input, len, 0, false};
    uint32_t scalar;
    int result;
    while ((result = next_scalar(&reader, &scalar)) > 0) {
        if (control(scalar) || scalar == ' ' || scalar == '?' || scalar == '#' || scalar == '\\') return false;
    }
    if (result < 0) return false;
    reader.index = 0;
    reader.percent = true;
    while ((result = next_scalar(&reader, &scalar)) > 0) {
        if (control(scalar)) return false;
    }
    return result == 0;
}
