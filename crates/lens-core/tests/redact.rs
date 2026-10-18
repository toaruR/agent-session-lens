use lens_core::redact::{sanitize, Field};

fn t(s: &str) -> String {
    sanitize(s, Field::Text)
}

#[test]
fn f16a_sk_key() {
    assert_eq!(t(&format!("sk-{}", "a".repeat(24))), "[REDACTED]");
    let s = format!("sk-{}", "a".repeat(19));
    assert_eq!(t(&s), s);
}

#[test]
fn f16b_github_token() {
    assert_eq!(t(&format!("ghp_{}", "a1".repeat(18))), "[REDACTED]");
    let s = format!("ghp_{}", "a".repeat(35));
    assert_eq!(t(&s), s);
}

#[test]
fn f16c_aws_key_id() {
    assert_eq!(t(&format!("AKIA{}", "A0".repeat(8))), "[REDACTED]");
    let s = format!("AKIA{}", "A".repeat(15));
    assert_eq!(t(&s), s);
}

#[test]
fn f16d_named_secret_assignment() {
    assert_eq!(
        t("ANTIGRAVITY_CSRF_TOKEN=abc"),
        "ANTIGRAVITY_CSRF_TOKEN=[REDACTED]"
    );
    assert_eq!(t("TOKEN_COUNT=5"), "TOKEN_COUNT=[REDACTED]");
}

#[test]
fn f30_truncate_text_2000() {
    let a = "x".repeat(2000);
    assert_eq!(t(&a), a);
    let b = "x".repeat(2001);
    assert_eq!(t(&b), a);
}

#[test]
fn f30_truncate_output_1000_head_tail() {
    let a = "y".repeat(2000);
    assert_eq!(sanitize(&a, Field::Output), a);
    let b = format!("{}{}{}", "h".repeat(1000), "m", "t".repeat(1000));
    let r = sanitize(&b, Field::Output);
    assert_eq!(r.chars().count(), 2000);
    assert_eq!(r, format!("{}{}", "h".repeat(1000), "t".repeat(1000)));
}
