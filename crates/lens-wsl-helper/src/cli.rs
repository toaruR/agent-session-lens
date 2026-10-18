//! Helper CLI contract: argument parsing and exit codes.
use lens_core::consts::PROTOCOL_VERSION;
use std::io::Write;
use std::path::Path;

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_PROTOCOL: i32 = 3;
pub const EXIT_ROOT: i32 = 4;
pub const EXIT_STDOUT: i32 = 5;

fn fail(err: &mut dyn Write, code: i32, name: &str) -> i32 {
    let _ = write!(err, "{name}");
    let _ = err.flush();
    code
}

fn is_abs(p: &str) -> bool {
    p.starts_with('/') || Path::new(p).is_absolute()
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        Some("--version") if args.len() == 1 => {
            let line = format!(
                "lens-wsl-helper {} protocol {}\n",
                env!("CARGO_PKG_VERSION"),
                PROTOCOL_VERSION
            );
            if out.write_all(line.as_bytes()).and_then(|_| out.flush()).is_err() {
                return fail(err, EXIT_STDOUT, "stdout_failed");
            }
            EXIT_OK
        }
        Some("serve") => serve(&args[1..], out, err),
        _ => fail(err, EXIT_USAGE, "usage_error"),
    }
}

fn serve(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut protocol: Option<u32> = None;
    let mut roots: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--protocol" => {
                match args.get(i + 1).and_then(|v| v.parse::<u32>().ok()) {
                    Some(v) => protocol = Some(v),
                    None => return fail(err, EXIT_USAGE, "usage_error"),
                }
                i += 2;
            }
            "--roots" => {
                i += 1;
                while i < args.len() && !args[i].starts_with("--") {
                    if !is_abs(&args[i]) {
                        return fail(err, EXIT_USAGE, "usage_error");
                    }
                    roots.push(args[i].clone());
                    i += 1;
                }
            }
            _ => return fail(err, EXIT_USAGE, "usage_error"),
        }
    }
    let Some(protocol) = protocol else {
        return fail(err, EXIT_USAGE, "usage_error");
    };
    if protocol != PROTOCOL_VERSION {
        return fail(err, EXIT_PROTOCOL, "protocol_unsupported");
    }
    if !roots.is_empty() && !roots.iter().any(|r| std::fs::read_dir(r).is_ok()) {
        return fail(err, EXIT_ROOT, "root_unreadable");
    }
    // Stub: streaming is implemented by the serve task. Probe stdout so a
    // broken pipe maps to exit 5.
    if out.flush().is_err() {
        return fail(err, EXIT_STDOUT, "stdout_failed");
    }
    EXIT_OK
}
