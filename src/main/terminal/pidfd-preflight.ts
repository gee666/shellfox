export const PIDFD_READY = 'SHELLFOX_PIDFD_READY_V1';
export const PIDFD_PREFLIGHT = `import os, signal, select
assert callable(getattr(os, 'pidfd_open', None)) and callable(getattr(signal, 'pidfd_send_signal', None))
assert os.getuid() == os.geteuid()
open('/proc/sys/kernel/random/boot_id').read()
open('/proc/self/stat').read()
fd = os.pidfd_open(os.getpid(), 0)
try: signal.pidfd_send_signal(fd, 0, None, 0)
finally: os.close(fd)
print('${PIDFD_READY}')
`;
