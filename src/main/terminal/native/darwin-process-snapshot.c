/* Read-only macOS process snapshots. Build against the target macOS SDK, -lproc.
 * No signals, process termination, shell execution, or hardcoded kinfo_proc offsets.
 * Protocol v1: JSON on stdout, capabilities/snapshot only. */
#define _DARWIN_C_SOURCE 1
#if !defined(__APPLE__)
#error This helper requires the macOS SDK and libproc.
#endif
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/proc.h>
#include <sys/sysctl.h>
#include <unistd.h>
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__arm64__) || defined(__aarch64__)
#define HELPER_ARCH "arm64"
#elif defined(__x86_64__)
#define HELPER_ARCH "x64"
#else
#error Unsupported macOS architecture.
#endif

#define MAX_ROWS 20000
#define MAX_FIELD 65536
#define MAX_ARGS 4096
#define MAX_OUTPUT 8388608
#define MARKER_KEY "SHELLFOX_TERMINAL_MARKER="
#define SOURCE "libproc+numeric-sysctl"

static char *output;
static size_t used;
static bool output_failed;
static void emit(const char *format, ...) __attribute__((format(printf, 1, 2)));
static void emit(const char *format, ...) {
    if (output_failed) return;
    va_list args;
    va_start(args, format);
    int n = vsnprintf(output + used, MAX_OUTPUT + 1 - used, format, args);
    va_end(args);
    if (n < 0 || (size_t)n > MAX_OUTPUT - used) { output_failed = true; return; }
    used += (size_t)n;
}
/* Reject invalid UTF-8 metadata instead of changing argv/path bytes by replacement. */
static bool utf8(const char *text) {
    const unsigned char *p = (const unsigned char *)text;
    while (*p) {
        unsigned c = *p++;
        if (c < 0x80) continue;
        unsigned count, value, minimum;
        if (c >= 0xc2 && c <= 0xdf) { count = 1; value = c & 0x1f; minimum = 0x80; }
        else if (c >= 0xe0 && c <= 0xef) { count = 2; value = c & 0x0f; minimum = 0x800; }
        else if (c >= 0xf0 && c <= 0xf4) { count = 3; value = c & 7; minimum = 0x10000; }
        else return false;
        for (unsigned i = 0; i < count; i++) {
            if (!*p || (*p & 0xc0) != 0x80) return false;
            value = (value << 6) | (*p++ & 0x3f);
        }
        if (value < minimum || value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff)) return false;
    }
    return true;
}
static void string(const char *value) {
    if (!value) { emit("null"); return; }
    emit("\"");
    for (const unsigned char *p = (const unsigned char *)value; *p; p++) {
        if (*p == '"' || *p == '\\') emit("\\%c", *p);
        else if (*p < 0x20) emit("\\u%04x", (unsigned)*p);
        else emit("%c", *p);
    }
    emit("\"");
}
static bool bsd_info(pid_t pid, struct proc_bsdinfo *info) {
    memset(info, 0, sizeof(*info));
    int n = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, info, (int)sizeof(*info));
    return n == (int)sizeof(*info) && info->pbi_pid == (uint32_t)pid &&
        info->pbi_start_tvsec > 0 && info->pbi_start_tvusec < 1000000 &&
        info->pbi_status >= SIDL && info->pbi_status <= SZOMB;
}
/* The SDK supplies the struct layout. The kernel-returned byte count, PID, birth,
 * credentials and parent/group corroborate libproc, before AND after metadata. */
static bool corroborate(pid_t pid, const struct proc_bsdinfo *info) {
    int mib[4] = { CTL_KERN, KERN_PROC, KERN_PROC_PID, pid };
    struct kinfo_proc record;
    memset(&record, 0, sizeof(record));
    size_t size = sizeof(record);
    if (sysctl(mib, 4, &record, &size, NULL, 0) != 0 || size != sizeof(record)) return false;
    return record.kp_proc.p_pid == pid && record.kp_proc.p_starttime.tv_sec > 0 &&
        (uint64_t)record.kp_proc.p_starttime.tv_sec == info->pbi_start_tvsec &&
        record.kp_proc.p_starttime.tv_usec >= 0 &&
        (uint64_t)record.kp_proc.p_starttime.tv_usec == info->pbi_start_tvusec &&
        record.kp_eproc.e_ppid == (pid_t)info->pbi_ppid && record.kp_eproc.e_pgid == (pid_t)info->pbi_pgid &&
        record.kp_eproc.e_ucred.cr_uid == info->pbi_uid && record.kp_eproc.e_pcred.p_ruid == info->pbi_ruid &&
        record.kp_eproc.e_pcred.p_svuid == info->pbi_svuid;
}
static bool stable(const struct proc_bsdinfo *a, const struct proc_bsdinfo *b) {
    return a->pbi_pid == b->pbi_pid && a->pbi_start_tvsec == b->pbi_start_tvsec &&
        a->pbi_start_tvusec == b->pbi_start_tvusec && a->pbi_ppid == b->pbi_ppid &&
        a->pbi_pgid == b->pbi_pgid && a->pbi_uid == b->pbi_uid && a->pbi_ruid == b->pbi_ruid &&
        a->pbi_svuid == b->pbi_svuid && ((a->pbi_flags ^ b->pbi_flags) & PROC_FLAG_PSUGID) == 0;
}
static bool same_user(const struct proc_bsdinfo *info) {
    uid_t uid = getuid();
    return uid == geteuid() && info->pbi_uid == uid && info->pbi_ruid == uid && info->pbi_svuid == uid &&
        !(info->pbi_flags & (PROC_FLAG_PSUGID | PROC_FLAG_INEXIT)) && info->pbi_status != SZOMB;
}
static bool process_path(pid_t pid, char *path) {
    memset(path, 0, PROC_PIDPATHINFO_MAXSIZE);
    int n = proc_pidpath(pid, path, PROC_PIDPATHINFO_MAXSIZE);
    return n > 0 && path[0] == '/' && memchr(path, 0, PROC_PIDPATHINFO_MAXSIZE) != NULL && utf8(path);
}
struct args_info {
    char bytes[MAX_FIELD];
    char *argv[MAX_ARGS];
    int argc;
    const char *marker;
};
static char *next_string(char *bytes, size_t length, size_t *offset) {
    if (*offset >= length) return NULL;
    char *start = bytes + *offset;
    char *end = memchr(start, 0, length - *offset);
    if (!end || !utf8(start)) return NULL;
    *offset = (size_t)(end - bytes) + 1;
    return start;
}
static bool process_args(pid_t pid, struct args_info *args) {
    int mib[3] = { CTL_KERN, KERN_PROCARGS2, pid };
    size_t needed = 0;
    if (sysctl(mib, 3, NULL, &needed, NULL, 0) != 0 || needed < sizeof(int) + 2 || needed > MAX_FIELD) return false;
    size_t length = sizeof(args->bytes);
    if (sysctl(mib, 3, args->bytes, &length, NULL, 0) != 0 || length < sizeof(int) + 2 || length > MAX_FIELD) return false;
    memcpy(&args->argc, args->bytes, sizeof(int));
    if (args->argc < 1 || args->argc > MAX_ARGS) return false;
    size_t offset = sizeof(int);
    char *exec_path = next_string(args->bytes, length, &offset);
    if (!exec_path || exec_path[0] != '/') return false;
    /* KERN_PROCARGS2 aligns argv after the saved exec path. Empty argv entries
     * AFTER argv[0] are preserved. Empty/ambiguous argv[0] is unsupported. */
    while (offset < length && args->bytes[offset] == 0) offset++;
    for (int i = 0; i < args->argc; i++) {
        args->argv[i] = next_string(args->bytes, length, &offset);
        if (!args->argv[i]) return false;
    }
    if (!args->argv[0][0]) return false;
    args->marker = NULL;
    unsigned markers = 0;
    while (offset < length) {
        char *entry = next_string(args->bytes, length, &offset);
        if (!entry) return false;
        if (!strncmp(entry, MARKER_KEY, sizeof(MARKER_KEY) - 1)) {
            markers++;
            args->marker = entry + sizeof(MARKER_KEY) - 1;
            if (strlen(args->marker) > 1024) return false;
        }
    }
    if (markers != 1) args->marker = NULL; /* absent, duplicate or redacted env */
    return true;
}
/* Fixed allocation, kernel API count checked independently of size query. */
static int enumerate(pid_t *pids) {
    int estimate = proc_listallpids(NULL, 0);
    if (estimate <= 0 || estimate >= MAX_ROWS) return -1;
    memset(pids, 0, (MAX_ROWS + 1) * sizeof(*pids));
    int count = proc_listallpids(pids, (MAX_ROWS + 1) * (int)sizeof(*pids));
    if (count <= 0 || count >= MAX_ROWS) return -1;
    for (int i = 0; i < count; i++) {
        if (pids[i] < 0) return -1;
        for (int j = 0; j < i; j++) if (pids[i] && pids[i] == pids[j]) return -1;
    }
    return count;
}
static const char *preflight(struct args_info *args, pid_t *pids, bool marker_required) {
    if (getuid() != geteuid()) return "Same-user native tracking requires matching real and effective helper UID.";
    struct proc_bsdinfo a, b;
    pid_t pid = getpid();
    if (!bsd_info(pid, &a) || !corroborate(pid, &a)) return "libproc/numeric KERN_PROC_PID self identity preflight failed.";
    char path[PROC_PIDPATHINFO_MAXSIZE];
    if (!process_path(pid, path)) return "libproc executable-path self preflight failed.";
    if (!process_args(pid, args)) return "Numeric KERN_PROCARGS2 self argv preflight failed or exceeded the field bound.";
    const char *expected = getenv("SHELLFOX_TERMINAL_MARKER");
    if (marker_required && (!expected || !args->marker || strcmp(expected, args->marker))) return "Numeric KERN_PROCARGS2 environment-marker self preflight failed.";
    if (getsid(pid) <= 0 || !bsd_info(pid, &b) || !corroborate(pid, &b) || !stable(&a, &b)) return "Native self identity changed during preflight.";
    int count = enumerate(pids);
    if (count < 0) return "libproc PID enumeration preflight failed or exceeded the row bound.";
    bool found = false;
    for (int i = 0; i < count; i++) if (pids[i] == pid) found = true;
    return found ? NULL : "libproc enumeration omitted the helper's own PID.";
}
static void envelope(const char *kind) {
    emit("{\"protocol\":1,\"platform\":\"darwin\",\"arch\":\"%s\",\"source\":\"%s\",\"ownerUid\":%u,\"kind\":", HELPER_ARCH, SOURCE, (unsigned)getuid());
    string(kind);
}
static void unknown_row(pid_t pid) {
    emit("{\"pid\":%d,\"parentPid\":0,\"pgid\":null,\"sid\":null,\"uid\":null,\"realUid\":null,\"savedUid\":null,\"startSeconds\":null,\"startMicroseconds\":null,\"identityVerified\":false,\"accessible\":false,\"executable\":null,\"argv\":null,\"marker\":null}", pid);
}
static bool snapshot(struct args_info *args, pid_t *pids) {
    int count = enumerate(pids);
    if (count < 0) return false;
    envelope("snapshot"); emit(",\"processes\":[");
    bool first = true, complete = true;
    for (int i = 0; i < count && !output_failed; i++) {
        pid_t pid = pids[i];
        if (pid <= 0) continue;
        struct proc_bsdinfo a, b;
        errno = 0;
        if (!bsd_info(pid, &a)) {
            /* ESRCH proves this enumerated process already vanished. Other failures
             * do not prove absence or ancestry, so enumeration stays incomplete. */
            if (errno == ESRCH || errno == ENOENT) continue;
            if (!first) emit(",");
            first = false;
            unknown_row(pid);
            complete = false;
            continue;
        }
        bool verified = corroborate(pid, &a);
        pid_t sid = getsid(pid);
        char path[PROC_PIDPATHINFO_MAXSIZE], after_path[PROC_PIDPATHINFO_MAXSIZE];
        bool own = same_user(&a), have_path = own && process_path(pid, path);
        bool have_args = own && process_args(pid, args);
        bool after_ok = bsd_info(pid, &b) && corroborate(pid, &b);
        verified = verified && after_ok && stable(&a, &b) && sid > 0 && getsid(pid) == sid;
        if (have_path && (!process_path(pid, after_path) || strcmp(path, after_path))) verified = false;
        /* A final birth read brackets the second path/SID lookup as well. */
        struct proc_bsdinfo final;
        verified = verified && bsd_info(pid, &final) && corroborate(pid, &final) && stable(&a, &final);
        bool accessible = verified && own && same_user(&final) && have_path;
        if (!first) emit(",");
        first = false;
        emit("{\"pid\":%d,\"parentPid\":%u,\"pgid\":%u,\"sid\":", pid, a.pbi_ppid, a.pbi_pgid);
        if (sid > 0) emit("%d", sid); else emit("null");
        emit(",\"uid\":%u,\"realUid\":%u,\"savedUid\":%u,\"startSeconds\":", (unsigned)a.pbi_uid, (unsigned)a.pbi_ruid, (unsigned)a.pbi_svuid);
        if (verified) emit("\"%" PRIu64 "\"", a.pbi_start_tvsec); else emit("null");
        emit(",\"startMicroseconds\":");
        if (verified) emit("\"%" PRIu64 "\"", a.pbi_start_tvusec); else emit("null");
        emit(",\"identityVerified\":%s,\"accessible\":%s,\"executable\":", verified ? "true" : "false", accessible ? "true" : "false");
        string(accessible ? path : NULL);
        char name[sizeof(a.pbi_name) + 1] = {0};
        if (a.pbi_name[0]) memcpy(name, a.pbi_name, sizeof(a.pbi_name));
        else memcpy(name, a.pbi_comm, sizeof(a.pbi_comm));
        emit(",\"processName\":"); string(accessible && utf8(name) ? name : NULL);
        emit(",\"argv\":");
        if (accessible && have_args) {
            emit("["); for (int j = 0; j < args->argc; j++) { if (j) emit(","); string(args->argv[j]); } emit("]");
        } else emit("null");
        emit(",\"marker\":"); string(accessible && have_args ? args->marker : NULL); emit("}");
    }
    emit("],\"complete\":%s,\"reason\":", complete ? "true" : "false");
    string(complete ? NULL : "Some native process identity/ancestry evidence was inaccessible."); emit("}\n");
    return !output_failed;
}
int main(int argc, char **argv) {
    bool capabilities = argc == 2 && !strcmp(argv[1], "--capabilities");
    bool take_snapshot = argc == 2 && !strcmp(argv[1], "--snapshot");
    if (!capabilities && !take_snapshot) return 64;
    output = malloc(MAX_OUTPUT + 1);
    struct args_info *args = calloc(1, sizeof(*args));
    pid_t *pids = calloc(MAX_ROWS + 1, sizeof(*pids));
    if (!output || !args || !pids) { free(output); free(args); free(pids); return 70; }
    const char *reason = preflight(args, pids, capabilities);
    if (capabilities) {
        envelope("capabilities"); emit(",\"available\":%s,\"exactBirth\":true,\"identityAccess\":true,\"enumeration\":true,\"argv\":true,\"environmentMarker\":true,\"termination\":false,\"reason\":", reason ? "false" : "true");
        string(reason); emit("}\n");
    } else if (reason) {
        envelope("snapshot"); emit(",\"complete\":false,\"processes\":[],\"reason\":"); string(reason); emit("}\n");
    } else if (!snapshot(args, pids)) {
        /* Never publish truncated JSON or a partial table as healthy enumeration. */
        used = 0;
        output_failed = false;
        envelope("snapshot");
        emit(",\"complete\":false,\"processes\":[],\"reason\":\"Native enumeration failed or exceeded its row/output bound.\"}\n");
    }
    bool ok = !output_failed && fwrite(output, 1, used, stdout) == used && fflush(stdout) == 0;
    free(output); free(args); free(pids);
    return ok ? 0 : 70;
}
