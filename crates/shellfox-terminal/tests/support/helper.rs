use std::io::{self, BufRead, Write};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    if let Some(code) = shellfox_terminal::supervisor_entry()? {
        std::process::exit(code as i32);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--short") => {
            println!("PTY_FINAL_MARKER");
            std::process::exit(7);
        }
        Some("--echo") => {
            println!("HOME={}", std::env::var("HOME")?);
            println!("CWD={}", std::env::current_dir()?.display());
            println!(
                "SECRET={}",
                std::env::var("SHELLFOX_TEST_SECRET").unwrap_or_else(|_| "absent".into())
            );
            io::stdout().flush()?;
            let line = io::stdin()
                .lock()
                .lines()
                .next()
                .transpose()?
                .unwrap_or_default();
            println!("REPLY={line}");
        }
        Some("--flood") => {
            let bytes = vec![b'x'; 4096];
            for _ in 0..600 {
                io::stdout().write_all(&bytes)?;
            }
            println!("FLOOD_FINAL");
        }
        Some("--leave-child") => {
            std::process::Command::new(std::env::current_exe()?)
                .arg("--delayed-child")
                .spawn()?;
            println!("ROOT_EXITING");
        }
        Some("--delayed-child") => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(
                "descendant-survived",
                b"unexpected if inside terminated Job",
            )?;
        }
        Some("--idle") => std::thread::sleep(Duration::from_secs(30)),
        _ => anyhow::bail!("unknown helper mode"),
    }
    Ok(())
}
