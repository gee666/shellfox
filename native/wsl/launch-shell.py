import os
import pwd
import sys

distro, uid, name, guest_cwd, bash_prompt, zsh_env = sys.argv[1:]
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
# shellfox completion, loaded by the same startup hooks as other Shellfox tabs.
# A hook that cannot be set up only costs completion.
kind = os.path.basename(shell)
if kind == 'bash':
    inherited = os.environ.get('PROMPT_COMMAND')
    os.environ['PROMPT_COMMAND'] = bash_prompt + ('\n' + inherited if inherited else '')
elif kind == 'zsh':
    try:
        import tempfile
        hook = tempfile.mkdtemp(prefix='shellfox-zsh-')
        with open(os.path.join(hook, '.zshenv'), 'x') as file:
            file.write(zsh_env)
        if 'ZDOTDIR' in os.environ:
            os.environ['SHELLFOX_ZDOTDIR'] = os.environ['ZDOTDIR']
        else:
            os.environ.pop('SHELLFOX_ZDOTDIR', None)
        os.environ['ZDOTDIR'] = hook
    except OSError as error:
        print('zsh tab starts without shellfox completion:', error, file=sys.stderr)
os.execv(shell, ['-' + os.path.basename(shell)])
