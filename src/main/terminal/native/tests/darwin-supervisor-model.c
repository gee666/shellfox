/* Deterministic libproc/session/guardian stubs. production.h is copied verbatim
 * from the shipped C functions by scripts/darwin-supervisor-model-test.mjs. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <limits.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <signal.h>
#define PROC_PIDTBSDINFO 3
#define PROC_FLAG_PSUGID 0x100
#define SSTOP 4
#define SZOMB 5
struct proc_bsdinfo {
    uint32_t pbi_pid, pbi_ppid, pbi_pgid, pbi_uid, pbi_ruid, pbi_svuid, pbi_flags, pbi_status;
    uint64_t pbi_start_tvsec, pbi_start_tvusec;
};
struct guardian { pid_t pid, group; int fd; bool joined, freeze_requested, frozen, kill_sent, failed; };
struct member { pid_t pid, group; struct proc_bsdinfo info; };
static struct guardian guardians[256];
static size_t guardian_count;
static pid_t supervisor_pid = 100, shell_pid = 200;
static bool closed_state, closing_state, shell_exited;
static int shell_exit_code;
static double shell_wait_ready;
static bool shell_wait_error;
static unsigned shell_waits;
static int shell_event_code, shell_event_status;
static const char *last_reason;
static char snapshot_reason[512];
static double clock_value;
static bool stopped, executing, transition, recovered;
static unsigned stopped_snapshots, reads, sid_reads, commands, rollback_count;
static char trace[4096];
static uint64_t birth;
static unsigned flags, uid, group;
static bool rollback_ok;
enum scenario { CLEAN, FREEZE_EXIT, FREEZE_SIDL, STARTUP_DELAY, DENIED, FREEZE_DENIED, ENUM_FAILED, RECYCLE_AT_SID, FLAG_CHANGE, VANISHED, FOREIGN, UNKNOWN_SID, SID_CHANGE, GROUP_CHANGE, UID_CHANGE, DUPLICATE, ZOMBIE };
static enum scenario scenario;
static double now(void) { return clock_value; }
static void sleep_tick(void) { clock_value += 0.005; }
static uid_t getuid(void) { return 501; }
static pid_t getpgrp(void) { return supervisor_pid; }
static pid_t getsid(pid_t pid) {
    if (pid == 0) return supervisor_pid;
    sid_reads++;
    if (!executing || scenario == VANISHED) { errno = ESRCH; return -1; }
    if (scenario == FOREIGN) return 999;
    if (scenario == UNKNOWN_SID) { errno = EPERM; return -1; }
    if (scenario == RECYCLE_AT_SID && sid_reads == 2) birth++;
    if (scenario == SID_CHANGE && sid_reads == 2) return 999;
    return supervisor_pid;
}
static int proc_listallpids(int *pids, int bytes) {
    assert(bytes >= 3 * (int)sizeof(*pids));
    if (scenario == ENUM_FAILED) { errno = EACCES; return 0; }
    pids[0] = supervisor_pid;
    if (!executing) return 1;
    pids[1] = 200;
    if (stopped && !transition) stopped_snapshots++;
    if (scenario == DUPLICATE) { pids[2] = 200; return 3; }
    return 2;
}
static int proc_pidinfo(pid_t pid, int flavor, uint64_t arg, void *buffer, int bytes) {
    assert(pid == 200 && flavor == PROC_PIDTBSDINFO && arg == 0);
    assert(bytes == (int)sizeof(struct proc_bsdinfo));
    reads++;
    if (scenario == DENIED || (scenario == FREEZE_DENIED && recovered)) { errno = EACCES; return 0; }
    if (scenario == STARTUP_DELAY && clock_value < 0.75) { errno = ESRCH; return 0; }
    /* An exit racing F cannot finish until R. Re-reading while stopped cannot
     * fix this, regardless of the number of passes or the freeze timeout. */
    if (transition) { errno = scenario == FREEZE_DENIED ? EACCES : ESRCH; return 0; }
    struct proc_bsdinfo *p = buffer;
    *p = (struct proc_bsdinfo){ .pbi_pid = 200, .pbi_ppid = 100, .pbi_pgid = group,
        .pbi_uid = uid, .pbi_ruid = 501, .pbi_svuid = 501, .pbi_flags = flags,
        .pbi_start_tvsec = birth, .pbi_start_tvusec = 123,
        .pbi_status = scenario == ZOMBIE ? SZOMB : scenario == FREEZE_SIDL && stopped && !recovered ? 1 : stopped ? SSTOP : 2 };
    if (scenario == FLAG_CHANGE && reads == 1) p->pbi_flags = PROC_FLAG_PSUGID;
    if (scenario == GROUP_CHANGE && reads == 2) p->pbi_pgid++;
    if (scenario == UID_CHANGE && reads == 2) p->pbi_svuid++;
    return bytes;
}
static int model_waitid(idtype_t type, id_t pid, siginfo_t *event, int options) {
    assert(type == P_PID && pid == (id_t)shell_pid);
    assert(options == (WEXITED | WNOHANG | WNOWAIT));
    shell_waits++;
    if (shell_wait_error) { errno = ECHILD; return -1; }
    if ((!executing || scenario == ZOMBIE) && clock_value >= shell_wait_ready) {
        event->si_pid = shell_pid; event->si_code = shell_event_code; event->si_status = shell_event_status;
    }
    return 0;
}
#define waitid model_waitid
static struct guardian *find_guardian(pid_t pgid) { return guardian_count && guardians[0].group == pgid ? &guardians[0] : NULL; }
static bool acquire_guardian(pid_t pgid, double deadline) { (void)pgid; (void)deadline; assert(!"unexpected new group"); return false; }
static bool protected_guardian(struct guardian *g) { return g->joined && !g->failed; }
static bool guardian_stopped(struct guardian *g) { (void)g; return stopped; }
static bool send_bytes(int fd, const void *bytes, size_t length, double deadline) {
    (void)fd; (void)deadline;
    assert(length == 1 && commands + 1 < sizeof(trace));
    char command = *(const char *)bytes;
    trace[commands++] = command; trace[commands] = 0;
    if (command == 'F') {
        stopped = true;
        if ((scenario == FREEZE_EXIT || scenario == FREEZE_DENIED) && !recovered) transition = true;
    } else if (command == 'K') {
        assert(!transition && stopped && stopped_snapshots >= 3);
        executing = false;
    } else assert(!"unexpected command");
    return true;
}
static bool continue_guardian(struct guardian *g) { (void)g; return true; }
static bool rollback_groups(void) {
    rollback_count++;
    if (!rollback_ok) return false;
    for (size_t i = 0; i < guardian_count; i++) {
        if (!guardians[i].freeze_requested || guardians[i].kill_sent) continue;
        assert(commands + 1 < sizeof(trace)); trace[commands++] = 'R'; trace[commands] = 0;
        stopped = false; transition = false; recovered = true; stopped_snapshots = 0;
        guardians[i].freeze_requested = false; guardians[i].frozen = false;
    }
    return true;
}
#include "production.h"
static void reset(enum scenario next) {
    scenario = next; clock_value = 0; stopped = false; executing = true;
    transition = false; recovered = false; rollback_ok = true;
    stopped_snapshots = reads = sid_reads = commands = rollback_count = 0;
    trace[0] = snapshot_reason[0] = 0; birth = 1000; flags = 0; uid = 501; group = 200;
    closed_state = closing_state = shell_exited = false; last_reason = NULL;
    shell_pid = 200; shell_exit_code = 0; shell_wait_ready = 0; shell_wait_error = false;
    shell_waits = 0; shell_event_code = CLD_KILLED; shell_event_status = SIGKILL;
    guardian_count = 1; guardians[0] = (struct guardian){ .pid = 201, .group = 200, .fd = 3, .joined = true };
}
static bool snapshot(void) {
    struct member members[4]; size_t count = 999;
    return session_snapshot(members, &count, now() + 1.0);
}
int main(void) {
    reset(FREEZE_EXIT);
    if (!cleanup_session()) {
        fprintf(stderr, "FAIL: frozen exit race did not recover; trace=%s; reason=%s\n", trace, last_reason);
        return 1;
    }
    assert(closed_state && recovered && strcmp(trace, "FRFK") == 0);
    reset(FREEZE_SIDL);
    assert(cleanup_session() && closed_state && recovered && strcmp(trace, "FRFK") == 0);
    puts("PASS: failed frozen read restores groups before fresh authenticated freeze/commit");

    reset(FREEZE_DENIED);
    assert(!cleanup_session() && !closed_state && !stopped && !strchr(trace, 'K'));
    assert(recovered && clock_value >= FREEZE_SECONDS && clock_value < FREEZE_SECONDS + 0.01);
    assert(last_reason && strstr(last_reason, "pid=200") && strstr(last_reason, "errno="));
    reset(STARTUP_DELAY);
    assert(cleanup_session() && closed_state && clock_value >= 0.75 && clock_value < FREEZE_SECONDS);
    reset(ENUM_FAILED);
    assert(!cleanup_session() && !closed_state && commands == 0 && strstr(last_reason, "enumeration bound"));
    reset(DENIED);
    assert(!cleanup_session() && !closed_state && commands == 0 && guardian_count == 1);
    assert(clock_value >= FREEZE_SECONDS && clock_value < FREEZE_SECONDS + 0.01);
    reset(FREEZE_EXIT); rollback_ok = false;
    assert(!cleanup_session() && !closed_state && !strchr(trace, 'K') && rollback_count == 1);
    puts("PASS: persistent denial and failed rollback never commit or release ownership");

    reset(CLEAN); assert(snapshot());
    reset(RECYCLE_AT_SID); assert(!snapshot());
    reset(FLAG_CHANGE); assert(!snapshot());
    reset(SID_CHANGE); assert(!snapshot());
    reset(GROUP_CHANGE); assert(!snapshot());
    reset(UID_CHANGE); assert(!snapshot());
    reset(DUPLICATE); assert(!snapshot());
    reset(CLEAN); flags = PROC_FLAG_PSUGID; assert(!snapshot());
    reset(CLEAN); uid = 0; assert(!snapshot());
    reset(CLEAN); group = UINT_MAX; assert(!snapshot());
    reset(CLEAN); group = (unsigned)supervisor_pid; assert(!snapshot());
    reset(CLEAN); group = 0; assert(!snapshot());
    reset(UNKNOWN_SID); assert(!snapshot() && strstr(snapshot_reason, "initial SID lookup"));
    puts("PASS: birth/SID/group/credential changes and inaccessible identities fail closed");

    struct member members[4]; size_t count;
    reset(VANISHED); assert(session_snapshot(members, &count, 1.0) && count == 0 && reads == 0);
    reset(FOREIGN); assert(session_snapshot(members, &count, 1.0) && count == 0 && reads == 0);
    reset(ZOMBIE); assert(session_snapshot(members, &count, 1.0) && count == 0);
    reset(CLEAN); executing = false; assert(cleanup_session() && closed_state && commands == 0);
    assert(shell_exited && shell_exit_code == 128 + SIGKILL && shell_waits == 1);
    puts("PASS: proven initial absence, foreign sessions, zombies and verified empty session");

    /* Both empty-session paths must wait for the unreaped direct child, not
     * synthesize shell_exited from a zombie/absent libproc row. */
    reset(ZOMBIE); shell_wait_ready = 0.05; shell_event_code = CLD_EXITED; shell_event_status = 7;
    assert(cleanup_session() && closed_state && shell_exited && shell_exit_code == 7);
    assert(clock_value >= 0.05 && clock_value < 0.06 && shell_waits > 1 && commands == 0);
    reset(CLEAN); shell_wait_ready = 0.08;
    assert(cleanup_session() && closed_state && shell_exited && shell_exit_code == 128 + SIGKILL);
    assert(clock_value >= 0.08 && clock_value < 0.09 && shell_waits > 1 && strcmp(trace, "FK") == 0);
    reset(ZOMBIE); shell_wait_ready = FREEZE_SECONDS - 0.02;
    assert(cleanup_session() && closed_state && shell_exited && clock_value < FREEZE_SECONDS);
    puts("PASS: zombie and post-commit empty snapshots wait for direct-child exit and preserve real status");

    reset(ZOMBIE); shell_wait_ready = FREEZE_SECONDS + 0.5;
    assert(!cleanup_session() && !closed_state && !shell_exited && commands == 0);
    assert(clock_value >= FREEZE_SECONDS && clock_value < FREEZE_SECONDS + 0.01);
    assert(closing_state && strstr(last_reason, "Direct shell child exit"));
    /* Retrying retains the same child; only its eventual real event closes. */
    shell_wait_ready = clock_value; shell_event_code = CLD_EXITED; shell_event_status = 7;
    assert(cleanup_session() && closed_state && shell_exited && shell_exit_code == 7);
    reset(CLEAN); shell_wait_ready = 20;
    assert(!cleanup_session() && !closed_state && !shell_exited && strcmp(trace, "FK") == 0);
    assert(clock_value >= CONFIRM_SECONDS && clock_value < CONFIRM_SECONDS + 0.03);
    assert(closing_state && strstr(last_reason, "Direct shell child exit"));
    reset(ZOMBIE); shell_wait_error = true;
    assert(!cleanup_session() && !closed_state && !shell_exited);
    assert(clock_value >= FREEZE_SECONDS && clock_value < FREEZE_SECONDS + 0.01);
    reset(VANISHED); /* Enumeration misses a still-live direct child. */
    assert(!cleanup_session() && !closed_state && !shell_exited && executing && commands == 0);
    assert(clock_value >= FREEZE_SECONDS && clock_value < FREEZE_SECONDS + 0.01);
    puts("PASS: missing exit events and waitid errors retain ownership within the original phase deadline");

    reset(CLEAN); executing = false; shell_pid = -1;
    assert(cleanup_session() && closed_state && !shell_exited && shell_waits == 0);
    return 0;
}
