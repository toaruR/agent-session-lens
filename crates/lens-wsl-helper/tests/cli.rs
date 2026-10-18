use std::io::Write;
use std::process::Command;

fn helper(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_lens-wsl-helper"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn f29a_usage_errors_exit_2() {
    let cases: [&[&str]; 5] = [
        &["serve"],
        &["serve", "--protocol", "x"],
        &["serve", "--protocol", "1", "--roots", "rel"],
        &["bogus"],
        &[],
    ];
    for c in cases {
        let (code, _, err) = helper(c);
        assert_eq!(code, 2, "{c:?}");
        assert_eq!(err, "usage_error", "{c:?}");
    }
}

#[test]
fn f29b_protocol_mismatch_exit_3() {
    let (code, _, err) = helper(&["serve", "--protocol", "2"]);
    assert_eq!(code, 3);
    assert_eq!(err, "protocol_unsupported");
}

#[test]
fn f29c_all_roots_unreadable_exit_4() {
    let (code, _, err) = helper(&["serve", "--protocol", "1", "--roots", "/no/such"]);
    assert_eq!(code, 4);
    assert_eq!(err, "root_unreadable");
}

struct FailWrite;
impl Write for FailWrite {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("fail"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("fail"))
    }
}

// cli.rs is compiled into this test crate directly (binary-only crate).
#[path = "../src/cli.rs"]
mod cli;

#[test]
fn f29d_stdout_failure_exit_5() {
    let mut err = Vec::new();
    let args = vec!["serve".to_string(), "--protocol".into(), "1".into()];
    assert_eq!(cli::run(&args, &mut FailWrite, &mut err), 5);
    assert_eq!(String::from_utf8(err).unwrap(), "stdout_failed");
}

#[test]
fn version_prints_one_line() {
    let (code, out, _) = helper(&["--version"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 1);
}
