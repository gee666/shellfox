import base64
import errno
import os

# Lists this account's processes that carry SHELLFOX_TERMINAL_MARKER, for tab
# status only. The host matches markers and agent rules; nothing is passed in.
# Format: a header, then one line per process, "E" for a process that could
# not be read, and "END". Fields are base64 or "-".
FIELD = 65536
ENVIRON = 1048576
MARKER = b'SHELLFOX_TERMINAL_MARKER='


def read(path, limit):
    with open(path, 'rb') as file:
        data = file.read(limit + 1)
    if len(data) > limit:
        raise OverflowError(path)
    return data


def stat(directory):
    data = read(directory + '/stat', FIELD)
    fields = data[data.rfind(b')') + 2:].split()
    return fields[0], fields[1], fields[19], data[data.find(b'(') + 1:data.rfind(b')')]


def encode(data):
    return '-' if data is None else base64.b64encode(data).decode('ascii')


uid = os.getuid()
if uid != os.geteuid():
    raise SystemExit(3)
pids = [name for name in os.listdir('/proc') if name.isdigit()]
if len(pids) > 20000:
    raise SystemExit(4)
lines = ['SHELLFOX_GUEST_PROCESSES_1']
for pid in pids:
    directory = '/proc/' + pid
    try:
        state, parent, ticks, name = stat(directory)
        status = read(directory + '/status', FIELD).splitlines()
        users = next(line.split()[1:3] for line in status if line.startswith(b'Uid:'))
    except OSError as error:
        if error.errno not in (errno.ENOENT, errno.ESRCH):
            lines.append('E')
        continue
    except (OverflowError, ValueError, IndexError, StopIteration):
        lines.append('E')
        continue
    if state in (b'Z', b'X', b'x') or not all(int(user) == uid for user in users):
        continue
    try:
        environ = read(directory + '/environ', ENVIRON).split(b'\0')
    except OverflowError:
        lines.append('E')
        continue
    except OSError:
        # Like 0.9: an unreadable environment carries no marker.
        continue
    markers = b'\0'.join(entry for entry in environ if entry.startswith(MARKER))
    if not markers:
        continue
    accessible = True
    try:
        executable = os.readlink(os.fsencode(directory + '/exe'))
        if executable.endswith(b' (deleted)'):
            executable = executable[:-10]
    except OSError:
        executable = None
        accessible = False
    try:
        argv = read(directory + '/cmdline', FIELD) + b'\x001'
    except (OSError, OverflowError):
        argv = None
    try:
        after = stat(directory)
    except (OSError, OverflowError, IndexError):
        continue
    if after[0] in (b'Z', b'X', b'x'):
        continue
    if after[1:3] != (parent, ticks):
        accessible = False
    lines.append('\t'.join(['R', '1' if accessible else '0', encode(executable),
                            encode(argv), encode(markers), encode(name)]))
lines.append('END')
print('\n'.join(lines))
