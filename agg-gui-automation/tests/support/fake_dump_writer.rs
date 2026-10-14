//! A controllable stand-in for a dump writer, run by `thread_stack_dump_tests`
//! through `thread_stack_dump::run_dump_writer` as C#'s tests ran createdump
//! and `/bin/sleep`. It takes a mode, then the writer protocol's
//! `--name <dump path> <pid>` where the mode needs it:
//!
//! - `write --name <dump> <pid>` — logs createdump's start and finish lines
//!   for `<pid>` and writes a small dump file, then exits 0.
//! - `flood <bytes>` — writes `<bytes>` of log lines to stdout and as many
//!   to stderr (far more than a pipe holds), then exits 0.
//! - `hang` — never exits on its own.
//! - `fail <code>` — logs one line and exits with `<code>`.

use std::io::Write;
use std::time::{Duration, Instant};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mode = arguments.first().map(String::as_str).unwrap_or("");

    match mode {
        "write" => write_dump(&arguments[1..]),
        "flood" => {
            let bytes: usize = arguments
                .get(1)
                .and_then(|text| text.parse().ok())
                .expect("flood <bytes>");
            flood(&mut std::io::stdout().lock(), bytes, "stdout");
            flood(&mut std::io::stderr().lock(), bytes, "stderr");
        }
        "hang" => loop {
            std::thread::sleep(Duration::from_secs(600));
        },
        "fail" => {
            let code: i32 = arguments
                .get(1)
                .and_then(|text| text.parse().ok())
                .expect("fail <code>");
            println!("fake dump writer failing on purpose");
            std::process::exit(code);
        }
        other => {
            eprintln!("unknown mode {other:?}");
            std::process::exit(64);
        }
    }
}

fn write_dump(arguments: &[String]) {
    let started = Instant::now();
    let (dump_path, pid) = match arguments {
        [flag, dump_path, pid] if flag == "--name" => (dump_path, pid),
        _ => {
            eprintln!("expected --name <dump path> <pid>, got {arguments:?}");
            std::process::exit(64);
        }
    };

    println!("Gathering state for process {pid}");
    println!("Writing minidump to file {dump_path}");
    std::fs::write(dump_path, format!("fake dump of process {pid}\n")).expect("write the dump");
    println!(
        "Dump successfully written in {}ms",
        started.elapsed().as_millis()
    );
}

fn flood(out: &mut dyn Write, bytes: usize, stream: &str) {
    let mut written = 0;
    let mut line_number = 0;
    while written < bytes {
        let line = format!("{stream} verbose log line {line_number:08}\n");
        out.write_all(line.as_bytes()).expect("write a log line");
        written += line.len();
        line_number += 1;
    }
    out.flush().expect("flush the log");
}
