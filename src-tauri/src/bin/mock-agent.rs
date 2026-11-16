use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        if args.iter().any(|arg| arg == "--slow") {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        if args.iter().any(|arg| arg == "--bad-version") {
            eprintln!("unknown");
            return Ok(());
        }
        let version = args
            .iter()
            .find_map(|arg| arg.strip_prefix("--mock-version="))
            .unwrap_or("1.2.3");
        eprintln!("mock-agent/{version}");
        return Ok(());
    }
    println!(
        "\x1b[32mMock Agent ready\x1b[0m | cwd={}",
        std::env::current_dir()?.display()
    );
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line == "inspect launch" {
            println!(
                "fixture argv={args:?}\nfixture timeout={}\nfixture host={}\nfixture mode={}",
                std::env::var("GJC_OPENAI_STREAM_IDLE_TIMEOUT_MS").unwrap_or_default(),
                std::env::var("GJC_HOST_STATUS").unwrap_or_default(),
                std::env::var("DECK_FIXTURE_MODE").unwrap_or_default()
            );
            io::stdout().flush()?;
            continue;
        }
        if let Some(code) = line
            .strip_prefix("exit ")
            .and_then(|code| code.trim().parse().ok())
        {
            std::process::exit(code);
        }
        if let Some(state) = line.strip_prefix("host-status ") {
            if matches!(state, "working" | "attention" | "finished") {
                print!("\x1b]777;notify;Terax;gjc;{state}\x07");
                io::stdout().flush()?;
                continue;
            }
        }
        if let Some(count) = line
            .strip_prefix("stream ")
            .or_else(|| line.strip_prefix("scrollback "))
            .and_then(|count| count.trim().parse::<usize>().ok())
        {
            for index in 0..count.min(100_000) {
                println!("stream {index} {}", "x".repeat(80));
                if line.starts_with("scrollback ") {
                    io::stdout().flush()?;
                    std::thread::sleep(std::time::Duration::from_millis(12));
                }
            }
        } else {
            println!("echo: {line}");
        }
        io::stdout().flush()?;
    }
    Ok(())
}
