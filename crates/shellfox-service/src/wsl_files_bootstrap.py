# Fixed bootstrap: receive only the embedded ELF, then give the remaining pipes to it.
import os
import sys

distro, machine, uid, length = sys.argv[1:]
uid = int(uid)
if (uid == 0 or os.getuid() != uid or os.geteuid() != uid
        or os.uname().machine != machine
        or os.environ.get('WSL_DISTRO_NAME', '').casefold() != distro.casefold()):
    sys.exit('WSL Files requires the tab\'s saved launch account and architecture.')
remaining = int(length)
if not 0 < remaining <= 32 * 1024 * 1024:
    sys.exit('Invalid WSL Files worker length.')
fd = os.memfd_create('shellfox-wsl-files', os.MFD_CLOEXEC)
while remaining:
    chunk = os.read(0, min(remaining, 65536))
    if not chunk:
        raise EOFError('incomplete Shellfox Files binary')
    remaining -= len(chunk)
    data = memoryview(chunk)
    while data:
        data = data[os.write(fd, data):]
os.execve(fd, ['shellfox-wsl-files', '--stdio'], {})
