mod cli;
mod serve;

/// Panic contract (F29e): stderr is exactly `internal`, exit code 101.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        use std::io::Write;
        let mut e = std::io::stderr();
        let _ = e.write_all(b"internal");
        let _ = e.flush();
    }));
}

/// The env is read only when the feature is compiled in.
#[cfg(feature = "fault_inject")]
fn inject_fault() {
    if std::env::var("LENS_FAULT").as_deref() == Ok("panic") {
        panic!("fault injected");
    }
}

#[cfg(not(feature = "fault_inject"))]
fn inject_fault() {}

fn main() {
    install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stderr = std::io::stderr();
    if args.first().map(String::as_str) == Some("serve") {
        // Argument validation and the exit-code contract live in cli::run (output discarded).
        let code = cli::run(&args, &mut std::io::sink(), &mut stderr);
        if code != cli::EXIT_OK {
            std::process::exit(code);
        }
        inject_fault();
        let code = serve::run_stdio(&args[1..], &mut std::io::stdout(), &mut stderr);
        std::process::exit(code);
    }
    let code = cli::run(&args, &mut std::io::stdout(), &mut stderr);
    std::process::exit(code);
}
