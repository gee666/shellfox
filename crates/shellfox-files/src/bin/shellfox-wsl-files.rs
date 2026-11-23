fn main() -> std::process::ExitCode {
    #[cfg(target_os = "linux")]
    if std::env::args_os().skip(1).eq(["--stdio"]) {
        return match shellfox_files::serve_wsl_files(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("WSL Files worker: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    eprintln!("Run the Shellfox WSL Files worker on Linux with --stdio.");
    std::process::ExitCode::FAILURE
}
