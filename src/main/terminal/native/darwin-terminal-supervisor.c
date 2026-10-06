/* Per-tab macOS controlling-session supervisor. Build: xcrun clang -std=c11
 * -O2 -Wall -Wextra -Werror darwin-terminal-supervisor.c -lproc -o BINARY.
 * Only guardians signal groups, using kill(0, ...) after their OWN setpgid.
 * Parent may SIGCONT only a protected, unreaped direct guardian child.
 * No cached-PGID killpg, arbitrary PID kill, SIGCHLD auto-reaping, or Python. */
#define _DARWIN_C_SOURCE 1
#if !defined(__APPLE__)
#error This supervisor requires the macOS SDK.
#endif
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/proc.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/sysctl.h>
#include <fcntl.h>
#include <unistd.h>
#include <poll.h>
#include <signal.h>
#include <time.h>
#include <termios.h>
#include <errno.h>
#include <stdint.h>
#include <inttypes.h>
#include <limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__arm64__) || defined(__aarch64__)
#define ARCH "arm64"
#elif defined(__x86_64__)
#define ARCH "x64"
#else
#error Unsupported architecture.
#endif
#define MAX_PIDS 20000
#define MAX_GROUPS 256
#define MAX_REQUEST 128
#define MAX_REPLY 2048
#define FREEZE_SECONDS 5.0
#define CONFIRM_SECONDS 5.0
#define ROLLBACK_SECONDS 2.0
#define TOKEN_NAME "SHELLFOX_SUPERVISOR_TOKEN"
extern char **environ;

struct guardian {
    pid_t pid, group;
    int fd;
    bool joined, freeze_requested, frozen, kill_sent, failed;
};
struct member { pid_t pid, group; struct proc_bsdinfo info; };
static struct guardian guardians[MAX_GROUPS];
static size_t guardian_count;
static pid_t supervisor_pid, shell_pid;
static int listener = -1, directory_fd = -1, client_fd = -1;
static int shell_go = -1, shell_exec_error = -1;
static char token[65], socket_path[sizeof(((struct sockaddr_un *)0)->sun_path)];
static char socket_leaf[sizeof(socket_path)], shell_image[PATH_MAX], shell_birth[80];
static dev_t socket_device;
static ino_t socket_inode;
static bool socket_bound, shell_authenticated, shell_exited, closed_state, closing_state;
static int shell_exit_code;
static const char *last_reason;
static volatile sig_atomic_t close_requested, child_event;

static double now(void) {
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0) return 0;
    return (double)value.tv_sec + (double)value.tv_nsec / 1000000000.0;
}
static void sleep_tick(void) { (void)poll(NULL, 0, 5); }
static void on_close_signal(int number) { (void)number; close_requested = 1; }
static void on_child_signal(int number) { (void)number; child_event = 1; }
static bool set_handler(int number, void (*handler)(int)) {
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_handler = handler;
    sigemptyset(&action.sa_mask);
    return sigaction(number, &action, NULL) == 0;
}
static bool fd_flags(int fd) {
    return fcntl(fd, F_SETFD, FD_CLOEXEC) == 0 && fcntl(fd, F_SETFL, O_NONBLOCK) == 0;
}
static bool send_bytes(int fd, const void *bytes, size_t length, double deadline) {
    const char *p = bytes;
    while (length && now() < deadline) {
        ssize_t n = write(fd, p, length);
        if (n > 0) { p += n; length -= (size_t)n; continue; }
        if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) return false;
        struct pollfd event = { fd, POLLOUT, 0 };
        (void)poll(&event, 1, 5);
    }
    return length == 0;
}
static bool read_byte(int fd, char *value, double deadline) {
    while (now() < deadline) {
        ssize_t n = read(fd, value, 1);
        if (n == 1) return true;
        if (n == 0) return false;
        if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) return false;
        struct pollfd event = { fd, POLLIN, 0 };
        (void)poll(&event, 1, 5);
    }
    return false;
}
static void wipe(char *data, size_t length) {
    volatile char *p = data;
    while (length--) *p++ = 0;
}
static void strip_child_secrets(void) {
    /* Removing an env pointer alone leaves the nonce bytes in a forked guardian.
     * These are original writable exec environment strings. Wipe the entry then
     * compact the pointer array, without libc retaining an allocation/copy. */
    for (size_t i = 0; environ[i];) {
        const char prefix[] = "SHELLFOX_SUPERVISOR_";
        if (!strncmp(environ[i], prefix, sizeof(prefix) - 1)) {
            wipe(environ[i], strlen(environ[i]));
            for (size_t j = i; environ[j]; j++) environ[j] = environ[j + 1];
        } else i++;
    }
    wipe(token, sizeof(token)); wipe(socket_path, sizeof(socket_path)); wipe(socket_leaf, sizeof(socket_leaf));
}
static void close_parent_fds(int keep) {
    if (listener >= 0 && listener != keep) close(listener);
    if (directory_fd >= 0 && directory_fd != keep) close(directory_fd);
    if (client_fd >= 0 && client_fd != keep) close(client_fd);
    if (shell_go >= 0 && shell_go != keep) close(shell_go);
    if (shell_exec_error >= 0 && shell_exec_error != keep) close(shell_exec_error);
    for (size_t i = 0; i < guardian_count; i++) if (guardians[i].fd >= 0 && guardians[i].fd != keep) close(guardians[i].fd);
}
static bool info(pid_t pid, struct proc_bsdinfo *value) {
    memset(value, 0, sizeof(*value));
    return proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, value, (int)sizeof(*value)) == (int)sizeof(*value) &&
        value->pbi_pid == (uint32_t)pid && value->pbi_start_tvsec > 0 && value->pbi_start_tvusec < 1000000;
}
static bool same_birth(const struct proc_bsdinfo *a, const struct proc_bsdinfo *b) {
    return a->pbi_pid == b->pbi_pid && a->pbi_start_tvsec == b->pbi_start_tvsec && a->pbi_start_tvusec == b->pbi_start_tvusec;
}
static bool readable_member(const struct proc_bsdinfo *value) {
    uid_t uid = getuid();
    return value->pbi_uid == uid && value->pbi_ruid == uid && value->pbi_svuid == uid &&
        !(value->pbi_flags & PROC_FLAG_PSUGID);
}
static bool numeric_identity(pid_t pid, const struct proc_bsdinfo *value) {
    int mib[4] = { CTL_KERN, KERN_PROC, KERN_PROC_PID, pid };
    struct kinfo_proc record;
    memset(&record, 0, sizeof(record));
    size_t length = sizeof(record);
    return sysctl(mib, 4, &record, &length, NULL, 0) == 0 && length == sizeof(record) &&
        record.kp_proc.p_pid == pid && record.kp_proc.p_starttime.tv_sec > 0 &&
        (uint64_t)record.kp_proc.p_starttime.tv_sec == value->pbi_start_tvsec &&
        record.kp_proc.p_starttime.tv_usec >= 0 && (uint64_t)record.kp_proc.p_starttime.tv_usec == value->pbi_start_tvusec;
}
/* WNOWAIT is essential. An exited guardian remains our zombie child and its PID
 * cannot recycle. SIGCHLD only sets a flag. Reap exclusively after verified close. */
static bool child_status(pid_t pid, siginfo_t *event, bool stopped) {
    memset(event, 0, sizeof(*event));
    int flags = WEXITED | WNOHANG | WNOWAIT;
    if (stopped) flags |= WSTOPPED; /* non-reaping WUNTRACED semantics */
    int result;
    do { result = waitid(P_PID, (id_t)pid, event, flags); } while (result != 0 && errno == EINTR);
    return result == 0;
}
static bool exited_event(const siginfo_t *event) {
    return event->si_pid != 0 && (event->si_code == CLD_EXITED || event->si_code == CLD_KILLED || event->si_code == CLD_DUMPED);
}
static bool protected_guardian(struct guardian *g) {
    if (!g->joined || g->failed) return false;
    siginfo_t event;
    if (!child_status(g->pid, &event, false)) { g->failed = true; return false; }
    if (exited_event(&event)) {
        if (!g->kill_sent) g->failed = true;
        return false;
    }
    struct proc_bsdinfo value;
    if (!info(g->pid, &value) || value.pbi_ppid != (uint32_t)supervisor_pid ||
        value.pbi_pgid != (uint32_t)g->group || getsid(g->pid) != supervisor_pid || !readable_member(&value)) {
        g->failed = true; return false;
    }
    return true;
}
static bool continue_guardian(struct guardian *g) {
    if (!protected_guardian(g)) return false;
    /* The only parent-to-PID signal: an unreaped DIRECT guardian child. If it
     * exits after the check, it remains an unreaped zombie, never a recycled PID. */
    return kill(g->pid, SIGCONT) == 0;
}
static void guardian_main(int fd, pid_t group) {
    close_parent_fds(fd);
    strip_child_secrets();
    (void)set_handler(SIGHUP, SIG_IGN); (void)set_handler(SIGTERM, SIG_IGN);
    (void)set_handler(SIGINT, SIG_IGN); (void)set_handler(SIGQUIT, SIG_IGN);
    (void)set_handler(SIGTSTP, SIG_IGN); (void)set_handler(SIGTTIN, SIG_IGN);
    (void)set_handler(SIGTTOU, SIG_IGN); (void)set_handler(SIGPIPE, SIG_IGN);
    (void)set_handler(SIGCHLD, SIG_DFL);
    if (group <= 0 || group == supervisor_pid || getsid(0) != supervisor_pid || setpgid(0, group) != 0 || getpgrp() != group) {
        (void)write(fd, "E", 1); _exit(126);
    }
    int null_fd = open("/dev/null", O_RDWR);
    if (null_fd >= 0) { (void)dup2(null_fd, STDIN_FILENO); (void)dup2(null_fd, STDOUT_FILENO); (void)dup2(null_fd, STDERR_FILENO); if (null_fd > 2 && null_fd != fd) close(null_fd); }
    if (write(fd, "J", 1) != 1) _exit(126);
    for (;;) {
        char command;
        ssize_t n = read(fd, &command, 1);
        if (n < 0 && errno == EINTR) continue;
        /* Parent death closes its unique pipe endpoints. Same-group self kill
         * remains safe even then: this guardian still belongs to this group. */
        if (n == 0) { (void)kill(0, SIGKILL); _exit(126); }
        if (n != 1 || getsid(0) != supervisor_pid || getpgrp() != group) _exit(126);
        if (command == 'F') {
            if (kill(0, SIGSTOP) != 0) _exit(126);
            /* Returns only when the parent resumes this protected guardian.
             * Other members remain stopped until the next R or K command. */
        } else if (command == 'R') {
            if (kill(0, SIGCONT) != 0 || write(fd, "R", 1) != 1) _exit(126);
        } else if (command == 'K') {
            (void)kill(0, SIGKILL); _exit(126);
        } else _exit(126);
    }
}
static struct guardian *find_guardian(pid_t group) {
    for (size_t i = 0; i < guardian_count; i++) if (guardians[i].group == group) return &guardians[i];
    return NULL;
}
static bool acquire_guardian(pid_t group, double deadline) {
    if (group <= 0 || group == supervisor_pid || guardian_count >= MAX_GROUPS) return false;
    struct guardian *old = find_guardian(group);
    if (old) return protected_guardian(old);
    int pair[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) != 0) return false;
    (void)fcntl(pair[0], F_SETFD, FD_CLOEXEC); (void)fcntl(pair[1], F_SETFD, FD_CLOEXEC);
    pid_t child = fork();
    if (child == 0) { close(pair[0]); guardian_main(pair[1], group); }
    close(pair[1]);
    if (child < 0) { close(pair[0]); return false; }
    struct guardian *g = &guardians[guardian_count++];
    memset(g, 0, sizeof(*g)); g->pid = child; g->group = group; g->fd = pair[0];
    if (!fd_flags(g->fd)) { g->failed = true; return false; }
    char acknowledgement;
    if (!read_byte(g->fd, &acknowledgement, deadline) || acknowledgement != 'J') { g->failed = true; return false; }
    g->joined = true;
    return protected_guardian(g);
}
static bool session_snapshot(struct member *members, size_t *length, double deadline) {
    /* The supervisor is the live immutable session leader. A foreign or reused
     * session can never acquire this live SID. Each row is read twice. */
    if (getsid(0) != supervisor_pid || getpgrp() != supervisor_pid) return false;
    int *pids = calloc(MAX_PIDS + 1, sizeof(*pids));
    if (!pids) return false;
    int count = proc_listallpids(pids, (MAX_PIDS + 1) * (int)sizeof(*pids));
    if (count <= 0 || count > MAX_PIDS) { free(pids); return false; }
    *length = 0;
    for (int i = 0; i < count; i++) {
        if (now() >= deadline) { free(pids); return false; }
        pid_t pid = pids[i];
        if (pid <= 0 || pid == supervisor_pid) continue;
        pid_t sid = getsid(pid);
        if (sid < 0) { if (errno == ESRCH) continue; free(pids); return false; }
        if (sid != supervisor_pid) continue;
        struct proc_bsdinfo a, b;
        if (!info(pid, &a) || !info(pid, &b) || !same_birth(&a, &b) || a.pbi_pgid != b.pbi_pgid ||
            a.pbi_uid != b.pbi_uid || a.pbi_ruid != b.pbi_ruid || a.pbi_svuid != b.pbi_svuid || getsid(pid) != supervisor_pid) {
            /* A racing exit or membership change is not a safe empty snapshot. */
            free(pids); return false;
        }
        if (b.pbi_status == SZOMB) continue;
        if (!readable_member(&b) || b.pbi_pgid == 0 || b.pbi_pgid == (uint32_t)supervisor_pid) { free(pids); return false; }
        for (size_t j = 0; j < *length; j++) if (members[j].pid == pid) { free(pids); return false; }
        if (*length >= MAX_PIDS) { free(pids); return false; }
        members[*length].pid = pid; members[*length].group = (pid_t)b.pbi_pgid; members[*length].info = b; (*length)++;
    }
    free(pids); return true;
}
static bool guardian_stopped(struct guardian *g) {
    if (!protected_guardian(g)) return false;
    siginfo_t event;
    struct proc_bsdinfo value;
    /* WNOWAIT + actual SSTOP prevents a stale historical stop event from
     * masquerading as a currently stopped guardian. No reaping WUNTRACED race. */
    if (!child_status(g->pid, &event, true) || exited_event(&event) || !info(g->pid, &value)) return false;
    return event.si_pid == g->pid && event.si_code == CLD_STOPPED && value.pbi_status == SSTOP;
}
static bool rollback_groups(void) {
    double deadline = now() + ROLLBACK_SECONDS;
    bool finished[MAX_GROUPS] = { false }, ok = true;
    /* Dispatch every R before waiting for a slow group. K is irreversible once
     * queued; finish its protected dispatch instead of leaving its guard stopped. */
    for (size_t i = 0; i < guardian_count; i++) {
        struct guardian *g = &guardians[i];
        if (!g->freeze_requested) { finished[i] = true; continue; }
        if (!g->kill_sent && (!protected_guardian(g) || !send_bytes(g->fd, "R", 1, deadline))) {
            ok = false; finished[i] = true;
        }
    }
    for (;;) {
        bool pending = false;
        for (size_t i = 0; i < guardian_count; i++) {
            if (finished[i]) continue;
            struct guardian *g = &guardians[i];
            if (g->kill_sent) {
                siginfo_t event;
                if (child_status(g->pid, &event, false) && exited_event(&event)) { finished[i] = true; continue; }
            }
            /* A single CONT can arrive before F reaches kill(0,SIGSTOP).
             * Keep resuming our protected child until R/expected death completes. */
            if (!continue_guardian(g)) { ok = false; finished[i] = true; continue; }
            if (!g->kill_sent) {
                char response;
                ssize_t n = read(g->fd, &response, 1);
                if (n == 1 && response == 'R') {
                    g->freeze_requested = false; g->frozen = false; finished[i] = true; continue;
                }
                if (n == 0 || (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR)) {
                    ok = false; finished[i] = true; continue;
                }
            }
            pending = true;
        }
        if (!pending) break;
        if (now() >= deadline) { ok = false; break; }
        sleep_tick();
    }
    return ok;
}
static bool cleanup_session(void) {
    closing_state = true;
    struct member *members = calloc(MAX_PIDS, sizeof(*members));
    if (!members) { last_reason = "Owned cleanup allocation failed."; return false; }
    bool committed = false, result = false;
    double deadline = now() + FREEZE_SECONDS;
    unsigned stable = 0;
    for (unsigned pass = 0; pass < 100 && now() < deadline; pass++) {
        size_t count;
        if (!session_snapshot(members, &count, deadline)) { last_reason = "Owned session membership or credentials are inaccessible or changed."; goto finish; }
        if (count == 0) { closed_state = true; last_reason = NULL; free(members); return true; }
        bool added = false;
        for (size_t i = 0; i < count; i++) {
            struct guardian *g = find_guardian(members[i].group);
            if (!g) {
                if (!acquire_guardian(members[i].group, deadline)) { last_reason = "A job-control guardian could not join its owned session group."; goto finish; }
                added = true;
            }
        }
        for (size_t i = 0; i < guardian_count; i++) {
            struct guardian *g = &guardians[i];
            if (g->kill_sent) continue;
            if (!protected_guardian(g)) { last_reason = "An owned guardian exited unexpectedly; no numeric fallback is permitted."; goto finish; }
            if (!g->freeze_requested) {
                g->freeze_requested = true;
                if (!send_bytes(g->fd, "F", 1, deadline)) { last_reason = "Guardian freeze command failed."; goto finish; }
            }
            while (!guardian_stopped(g) && now() < deadline) sleep_tick();
            if (!guardian_stopped(g)) { last_reason = "Owned group freeze was not confirmed before its deadline."; goto finish; }
            g->frozen = true;
        }
        /* Re-read real session members, not just guardian stop acknowledgements.
         * This catches late forks, job-control group changes and partial stops. */
        size_t frozen_count;
        if (!session_snapshot(members, &frozen_count, deadline)) { last_reason = "Frozen session membership could not be authenticated."; goto finish; }
        bool all_frozen = true;
        for (size_t i = 0; i < frozen_count; i++) {
            struct guardian *g = find_guardian(members[i].group);
            if (!g || !g->frozen || members[i].info.pbi_status != SSTOP) { all_frozen = false; break; }
        }
        stable = !added && all_frozen ? stable + 1 : 0;
        if (stable >= 2) { result = true; break; }
        sleep_tick();
    }
    if (!result) { last_reason = "Owned job-control groups did not stabilize before the freeze deadline."; goto finish; }
    result = false;
    /* No asynchronous exit/exception at the commit boundary. Separate budget;
     * any subsequent failure still enters rollback for surviving guarded groups. */
    committed = true;
    deadline = now() + CONFIRM_SECONDS;
    for (size_t i = 0; i < guardian_count; i++) {
        struct guardian *g = &guardians[i];
        if (g->kill_sent) continue;
        if (!protected_guardian(g) || !send_bytes(g->fd, "K", 1, deadline)) { last_reason = "Owned group termination command failed."; goto finish; }
        g->kill_sent = true;
        if (!continue_guardian(g)) { last_reason = "The protected guardian could not execute group termination."; goto finish; }
    }
    while (now() < deadline) {
        size_t count;
        if (session_snapshot(members, &count, deadline) && count == 0) { closed_state = true; last_reason = NULL; result = true; break; }
        sleep_tick();
    }
    if (!result) last_reason = "Executing owned session members remain; closure is not confirmed.";
finish:
    free(members);
    if (!result) {
        /* Even after commit, surviving non-killed groups must be resumed. */
        if (!rollback_groups()) last_reason = committed ? "Cleanup was partial and a surviving group could not be restored; supervisor retained for retry." : "Cleanup refused and a frozen group could not be restored; supervisor retained for retry.";
    }
    return result;
}
static void observe_shell(void) {
    if (shell_pid <= 0 || shell_exited) return;
    siginfo_t event;
    if (!child_status(shell_pid, &event, false)) { last_reason = "Shell child identity is unavailable."; return; }
    if (exited_event(&event)) {
        shell_exited = true;
        shell_exit_code = event.si_code == CLD_EXITED ? event.si_status : 128 + event.si_status;
    }
}
static bool authenticate_shell(double deadline) {
    while (now() < deadline) {
        observe_shell();
        if (shell_exited) { last_reason = "Shell exited before its exec image could be authenticated."; return false; }
        struct proc_bsdinfo a, b;
        char path[PROC_PIDPATHINFO_MAXSIZE];
        memset(path, 0, sizeof(path));
        if (info(shell_pid, &a) && a.pbi_ppid == (uint32_t)supervisor_pid && a.pbi_pgid == (uint32_t)shell_pid &&
            getsid(shell_pid) == supervisor_pid && readable_member(&a) && numeric_identity(shell_pid, &a) &&
            proc_pidpath(shell_pid, path, sizeof(path)) > 0 && memchr(path, 0, sizeof(path)) && !strcmp(path, shell_image) &&
            info(shell_pid, &b) && same_birth(&a, &b) && readable_member(&b)) {
            (void)snprintf(shell_birth, sizeof(shell_birth), "darwin:%" PRIu64 ":%06" PRIu64, b.pbi_start_tvsec, b.pbi_start_tvusec);
            shell_authenticated = true; last_reason = NULL; return true;
        }
        sleep_tick();
    }
    last_reason = "Shell exec/image authentication timed out; no birth identity is invented.";
    return false;
}
static bool valid_token(const char *value) {
    if (!value || strlen(value) != 64) return false;
    for (size_t i = 0; i < 64; i++) if (!((value[i] >= '0' && value[i] <= '9') || (value[i] >= 'a' && value[i] <= 'f'))) return false;
    return true;
}
static bool valid_marker(const char *value) {
    if (!value || strlen(value) != 36) return false;
    for (size_t i = 0; i < 36; i++) {
        if (i == 8 || i == 13 || i == 18 || i == 23) { if (value[i] != '-') return false; }
        else if (!((value[i] >= '0' && value[i] <= '9') || (value[i] >= 'a' && value[i] <= 'f') || (value[i] >= 'A' && value[i] <= 'F'))) return false;
    }
    return true;
}
static bool bind_control(const char *path) {
    if (!path || path[0] != '/' || strlen(path) >= sizeof(socket_path)) return false;
    char parent[PATH_MAX];
    if (strlen(path) >= sizeof(parent)) return false;
    strcpy(parent, path);
    char *slash = strrchr(parent, '/');
    if (!slash || slash == parent || !slash[1] || !strcmp(slash + 1, ".") || !strcmp(slash + 1, "..")) return false;
    strcpy(socket_leaf, slash + 1); *slash = 0;
    char canonical[PATH_MAX];
    if (!realpath(parent, canonical) || strcmp(canonical, parent)) return false;
    directory_fd = open(parent, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    struct stat dir, existing;
    if (directory_fd < 0 || fstat(directory_fd, &dir) != 0 || !S_ISDIR(dir.st_mode) || dir.st_uid != getuid() || (dir.st_mode & 0777) != 0700) return false;
    if (fstatat(directory_fd, socket_leaf, &existing, AT_SYMLINK_NOFOLLOW) == 0 || errno != ENOENT) return false;
    listener = socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener < 0 || !fd_flags(listener)) return false;
    struct sockaddr_un address;
    memset(&address, 0, sizeof(address)); address.sun_family = AF_UNIX;
    strcpy(address.sun_path, path); address.sun_len = (uint8_t)sizeof(address);
    mode_t old_mask = umask(0077);
    int result = bind(listener, (struct sockaddr *)&address, sizeof(address));
    umask(old_mask);
    if (result != 0) return false;
    socket_bound = true; strcpy(socket_path, path);
    if (chmod(path, 0600) != 0 || fstatat(directory_fd, socket_leaf, &existing, AT_SYMLINK_NOFOLLOW) != 0 || !S_ISSOCK(existing.st_mode) || existing.st_uid != getuid()) return false;
    socket_device = existing.st_dev; socket_inode = existing.st_ino;
    return listen(listener, 8) == 0;
}
static void unlink_control(void) {
    if (socket_bound && directory_fd >= 0) {
        struct stat value;
        if (fstatat(directory_fd, socket_leaf, &value, AT_SYMLINK_NOFOLLOW) == 0 && S_ISSOCK(value.st_mode) && value.st_dev == socket_device && value.st_ino == socket_inode) (void)unlinkat(directory_fd, socket_leaf, 0);
    }
    if (listener >= 0) close(listener);
    if (directory_fd >= 0) close(directory_fd);
    listener = directory_fd = -1; wipe(token, sizeof(token));
}
static bool start_shell(char **arguments) {
    int ready[2], go[2], failure[2];
    if (pipe(ready) != 0) return false;
    if (pipe(go) != 0) { close(ready[0]); close(ready[1]); return false; }
    if (pipe(failure) != 0) { close(ready[0]); close(ready[1]); close(go[0]); close(go[1]); return false; }
    (void)fcntl(failure[1], F_SETFD, FD_CLOEXEC);
    shell_pid = fork();
    if (shell_pid == 0) {
        close(ready[0]); close(go[1]); close(failure[0]); close_parent_fds(-1); strip_child_secrets();
        if (setpgid(0, 0) != 0 || getsid(0) != supervisor_pid) _exit(126);
        if (write(ready[1], "S", 1) != 1) _exit(126);
        close(ready[1]);
        char value;
        if (read(go[0], &value, 1) != 1 || value != 'G') _exit(126);
        close(go[0]);
        (void)set_handler(SIGCHLD, SIG_DFL); (void)set_handler(SIGHUP, SIG_DFL);
        (void)set_handler(SIGTERM, SIG_DFL); (void)set_handler(SIGINT, SIG_DFL);
        (void)set_handler(SIGQUIT, SIG_DFL); (void)set_handler(SIGTSTP, SIG_DFL);
        (void)set_handler(SIGTTIN, SIG_DFL); (void)set_handler(SIGTTOU, SIG_DFL); (void)set_handler(SIGPIPE, SIG_DFL);
        sigset_t empty; sigemptyset(&empty); (void)sigprocmask(SIG_SETMASK, &empty, NULL);
        arguments[0] = shell_image;
        execv(shell_image, arguments);
        int error = errno; (void)write(failure[1], &error, sizeof(error)); _exit(127);
    }
    close(ready[1]); close(go[0]); close(failure[1]);
    if (shell_pid < 0) { close(ready[0]); close(go[1]); close(failure[0]); return false; }
    shell_go = go[1]; shell_exec_error = failure[0];
    (void)fd_flags(ready[0]); (void)fd_flags(shell_exec_error);
    char value;
    bool ok = read_byte(ready[0], &value, now() + 3.0) && value == 'S';
    close(ready[0]);
    if (!ok || !acquire_guardian(shell_pid, now() + 3.0) || tcsetpgrp(STDIN_FILENO, shell_pid) != 0 || !send_bytes(shell_go, "G", 1, now() + 1.0)) return false;
    close(shell_go); shell_go = -1;
    double deadline = now() + 3.0;
    for (;;) {
        int error;
        ssize_t n = read(shell_exec_error, &error, sizeof(error));
        if (n == 0) break; /* close-on-exec proves exec succeeded, not its image */
        if (n > 0 || (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) || now() >= deadline) {
            last_reason = "The launched shell could not exec its canonical image."; return false;
        }
        sleep_tick();
    }
    close(shell_exec_error); shell_exec_error = -1;
    return authenticate_shell(now() + 3.0);
}
static void json_string(char *buffer, size_t capacity, const char *value) {
    size_t n = 0;
    if (!value) { (void)snprintf(buffer, capacity, "null"); return; }
    if (capacity < 3) return;
    buffer[n++] = '"';
    for (const unsigned char *p = (const unsigned char *)value; *p && n + 7 < capacity; p++) {
        if (*p == '"' || *p == '\\') { buffer[n++] = '\\'; buffer[n++] = (char)*p; }
        else if (*p < 0x20) { int written = snprintf(buffer + n, capacity - n, "\\u%04x", (unsigned)*p); if (written != 6) break; n += 6; }
        else buffer[n++] = (char)*p;
    }
    buffer[n++] = '"'; buffer[n] = 0;
}
static void reply(int fd, bool ok) {
    observe_shell();
    char reason[1024], birth[128], code[32], body[MAX_REPLY];
    json_string(reason, sizeof(reason), last_reason); json_string(birth, sizeof(birth), shell_birth);
    if (shell_exited) (void)snprintf(code, sizeof(code), "%d", shell_exit_code); else strcpy(code, "null");
    int n = snprintf(body, sizeof(body), "{\"version\":1,\"ok\":%s,\"state\":\"%s\",\"supervisorPid\":%d,\"shellPid\":%d,\"shellBirth\":%s,\"shellAlive\":%s,\"exitCode\":%s,\"reason\":%s}\n",
        ok ? "true" : "false", closed_state ? "closed" : closing_state ? "closing" : "open", supervisor_pid, shell_pid > 0 ? shell_pid : 0, birth,
        shell_pid > 0 && !shell_exited ? "true" : "false", code, reason);
    if (n > 0 && (size_t)n < sizeof(body)) (void)send_bytes(fd, body, (size_t)n, now() + 1.0);
    wipe(body, sizeof(body));
}
static bool authenticate_request(int fd, bool *close_command) {
    uid_t uid; gid_t gid;
    if (getpeereid(fd, &uid, &gid) != 0 || uid != getuid()) return false;
    char request[MAX_REQUEST]; size_t length = 0;
    double deadline = now() + 1.0;
    while (length < sizeof(request) && now() < deadline) {
        char c;
        if (!read_byte(fd, &c, deadline)) break;
        if ((unsigned char)c > 127 || c == 0 || c == '\r') break;
        request[length++] = c;
        if (c == '\n') break;
    }
    bool format = (length == 74 || length == 73) && request[0] == '1' && request[1] == '\t' && request[66] == '\t' && request[length - 1] == '\n';
    unsigned difference = 0;
    if (format) for (size_t i = 0; i < 64; i++) difference |= (unsigned char)request[i + 2] ^ (unsigned char)token[i];
    bool status = format && length == 74 && !memcmp(request + 67, "STATUS\n", 7);
    bool close = format && length == 73 && !memcmp(request + 67, "CLOSE\n", 6);
    wipe(request, sizeof(request));
    *close_command = close;
    return format && difference == 0 && (status || close);
}
static void reap_after_verified_close(void) {
    /* No group can still execute now. This is the sole guardian reap site. */
    for (size_t i = 0; i < guardian_count; i++) {
        if (guardians[i].fd >= 0) close(guardians[i].fd);
        int status; (void)waitpid(guardians[i].pid, &status, WNOHANG);
    }
    observe_shell();
    if (shell_pid > 0) { int status; (void)waitpid(shell_pid, &status, WNOHANG); }
}
static const char *self_preflight(void) {
    if (getuid() != geteuid()) return "Supervisor real/effective UID mismatch.";
    if (now() <= 0) return "Monotonic supervisor deadlines are unavailable.";
    struct proc_bsdinfo a, b;
    if (!info(getpid(), &a) || !numeric_identity(getpid(), &a) || !readable_member(&a) ||
        !info(getpid(), &b) || !same_birth(&a, &b) || getsid(0) <= 0 || getpgrp() <= 0) return "Native exact identity/session APIs failed self preflight.";
    int *pids = calloc(MAX_PIDS + 1, sizeof(*pids));
    if (!pids) return "Native enumeration self preflight allocation failed.";
    int count = proc_listallpids(pids, (MAX_PIDS + 1) * (int)sizeof(*pids));
    bool found = false;
    if (count > 0 && count <= MAX_PIDS) for (int i = 0; i < count; i++) if (pids[i] == getpid()) found = true;
    free(pids);
    if (!found) return "Native bounded process enumeration failed self preflight.";
    int pair[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) != 0) return "Private Unix control socket API is unavailable.";
    uid_t uid; gid_t gid;
    bool peer = getpeereid(pair[0], &uid, &gid) == 0 && uid == getuid();
    close(pair[0]); close(pair[1]);
    return peer ? NULL : "Same-user Unix peer credential API failed self preflight.";
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--capabilities")) {
        const char *reason = self_preflight(); char encoded[512]; json_string(encoded, sizeof(encoded), reason);
        printf("{\"version\":1,\"platform\":\"darwin\",\"arch\":\"%s\",\"available\":%s,\"reason\":%s,\"ownedSession\":true,\"guardians\":true,\"termination\":true}\n", ARCH, reason ? "false" : "true", encoded);
        return 0;
    }
    if (argc < 4 || strcmp(argv[1], "--supervise")) return 64;
    supervisor_pid = getpid();
    const char *secret = getenv(TOKEN_NAME), *marker = getenv("SHELLFOX_TERMINAL_MARKER");
    if (!valid_token(secret) || !valid_marker(marker) || self_preflight() || getsid(0) != supervisor_pid || getpgrp() != supervisor_pid ||
        !isatty(STDIN_FILENO) || tcgetsid(STDIN_FILENO) != supervisor_pid || !realpath(argv[3], shell_image) || access(shell_image, X_OK) != 0) return 70;
    memcpy(token, secret, 65);
    (void)set_handler(SIGCHLD, on_child_signal); /* Never SIG_IGN or SA_NOCLDWAIT. */
    (void)set_handler(SIGHUP, on_close_signal); (void)set_handler(SIGTERM, on_close_signal);
    (void)set_handler(SIGALRM, on_close_signal); /* Never die/throw at a freeze/commit boundary. */
    (void)set_handler(SIGINT, SIG_IGN); (void)set_handler(SIGQUIT, SIG_IGN);
    (void)set_handler(SIGTSTP, SIG_IGN); (void)set_handler(SIGTTIN, SIG_IGN);
    (void)set_handler(SIGTTOU, SIG_IGN); (void)set_handler(SIGPIPE, SIG_IGN);
    if (!bind_control(argv[2])) { unlink_control(); return 70; }
    if (!start_shell(&argv[3])) {
        closing_state = true; close_requested = 1;
        if (shell_go >= 0) { close(shell_go); shell_go = -1; }
        if (shell_exec_error >= 0) { close(shell_exec_error); shell_exec_error = -1; }
        if (!last_reason) last_reason = "Canonical shell startup/guardian preparation failed.";
    }
    double automatic_retry = 0;
    while (!closed_state) {
        observe_shell(); child_event = 0;
        if ((shell_exited || close_requested) && now() >= automatic_retry) {
            if (cleanup_session()) break;
            automatic_retry = now() + 1.0;
        }
        /* Do not read terminal input. Detect master loss even if no HUP was delivered. */
        struct pollfd events[2] = { { listener, POLLIN, 0 }, { STDIN_FILENO, 0, 0 } };
        (void)poll(events, 2, 50);
        if (events[1].revents & (POLLHUP | POLLERR | POLLNVAL)) close_requested = 1;
        if (!(events[0].revents & POLLIN)) continue;
        client_fd = accept(listener, NULL, NULL);
        if (client_fd < 0) continue;
        (void)fd_flags(client_fd);
        int no_sigpipe = 1; (void)setsockopt(client_fd, SOL_SOCKET, SO_NOSIGPIPE, &no_sigpipe, sizeof(no_sigpipe));
        bool close_command;
        if (authenticate_request(client_fd, &close_command)) {
            observe_shell();
            bool ok = true;
            if (close_command || shell_exited) ok = cleanup_session();
            else if (!shell_authenticated && !closing_state) ok = authenticate_shell(now() + 3.0);
            else if (closing_state) ok = false;
            reply(client_fd, ok);
        }
        close(client_fd); client_fd = -1;
    }
    /* An actual shell exit event is delivered by node-pty only after this point. */
    observe_shell(); reap_after_verified_close(); unlink_control();
    return shell_exited ? shell_exit_code : 0;
}
