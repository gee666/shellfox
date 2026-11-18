# Fixed bootstrap passed only to an explicitly selected trusted Python with -I -S.
# No guest shell, site packages, user imports or profile evaluation occurs here.
import hashlib
import json
import os
import stat
import sys
import types


def load():
    if not sys.flags.isolated or not sys.flags.no_site or sys.platform != 'linux':
        raise RuntimeError('isolated Linux interpreter required')
    executable = os.path.realpath(sys.executable)
    info = os.stat(executable)
    if info.st_uid not in (0, os.geteuid()) or info.st_mode & 0o022:
        raise RuntimeError('untrusted interpreter')
    manifest = json.loads(sys.argv[1])
    expected = ['shellfox_wsl_wire', 'shellfox_wsl_tree', 'shellfox_wsl_child', 'shellfox_wsl_guardian']
    if not isinstance(manifest, list) or len(manifest) != 4:
        raise RuntimeError('invalid bundle')
    for item, name in zip(manifest, expected):
        if set(item) != {'name', 'path', 'sha256'} or item['name'] != name:
            raise RuntimeError('invalid module')
        path = item['path']
        if not isinstance(path, str) or not path.startswith('/') or '..' in path.split('/'):
            raise RuntimeError('invalid module path')
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
        try:
            meta = os.fstat(fd)
            if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1 or meta.st_uid not in (0, os.geteuid()) or meta.st_mode & 0o022:
                raise RuntimeError('untrusted module')
            source = os.read(fd, 128 * 1024 + 1)
            if len(source) > 128 * 1024 or hashlib.sha256(source).hexdigest() != item['sha256']:
                raise RuntimeError('changed module')
        finally:
            os.close(fd)
        module = types.ModuleType(name)
        module.__file__ = path
        sys.modules[name] = module
        exec(compile(source, path, 'exec'), module.__dict__)
    sys.modules[expected[-1]].main()


try:
    load()
except BaseException:
    # Never serialize argv, credentials, native diagnostics or terminal bytes.
    sys.exit(1)
