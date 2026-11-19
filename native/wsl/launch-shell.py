import os
import pwd
import sys

distro, uid, name, guest_cwd = sys.argv[1:]
uid = int(uid)
if (uid == 0 or os.getuid() != uid or os.geteuid() != uid
        or os.environ.get('WSL_DISTRO_NAME', '').casefold() != distro.casefold()):
    sys.exit('WSL terminal launch account does not match the selected tab.')
account = pwd.getpwuid(uid)
if account.pw_name != name:
    sys.exit('WSL terminal launch account name changed.')
if guest_cwd:
    os.chdir(guest_cwd)
shell = account.pw_shell or '/bin/sh'
os.execv(shell, ['-' + os.path.basename(shell)])
