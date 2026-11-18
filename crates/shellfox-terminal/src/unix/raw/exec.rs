//! Direct execve prevents libc/Rust's ENOEXEC fallback to an implicit shell.
use super::RawCommand;
use anyhow::Result;
use std::{
    ffi::CString,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
};
pub(super) fn spawn(command: &RawCommand) -> Result<Child> {
    command.validate()?;
    let executable = CString::new(command.executable.as_str())?;
    let mut arguments = vec![executable.clone()];
    for argument in &command.args {
        arguments.push(CString::new(argument.as_str())?);
    }
    let mut environment = Vec::new();
    for (key, value) in &command.env {
        environment.push(CString::new(format!("{key}={value}"))?);
    }
    let mut builder = Command::new(&command.executable);
    builder
        .args(&command.args)
        .current_dir(&command.cwd)
        .env_clear()
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // RawCommand's validated4096-argument/1024-environment bounds allow stack
    // pointer arrays. All strings are allocated BEFORE fork. Only signal/execve
    // and fixed-array stores execute in the child; no allocation or locks.
    unsafe {
        builder.pre_exec(move || {
            for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
                if libc::signal(signal, libc::SIG_DFL) == libc::SIG_ERR {
                    return Err(std::io::Error::last_os_error());
                }
            }
            let mut argv = [std::ptr::null(); 4098];
            for (index, value) in arguments.iter().enumerate() {
                argv[index] = value.as_ptr();
            }
            let mut envp = [std::ptr::null(); 1025];
            for (index, value) in environment.iter().enumerate() {
                envp[index] = value.as_ptr();
            }
            libc::execve(executable.as_ptr(), argv.as_ptr(), envp.as_ptr());
            // A pre_exec error uses std's original exec-error pipe and aborts spawn.
            // It never reaches std's later execvp/posix_spawn shell fallback.
            Err(std::io::Error::last_os_error())
        });
    }
    Ok(builder.spawn()?)
}
