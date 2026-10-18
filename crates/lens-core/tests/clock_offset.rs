use lens_core::clock_offset::ClockOffset;

fn with_offset(o: i64) -> ClockOffset {
    let mut c = ClockOffset::new();
    c.record(1_000_000 + o, 1_000_000);
    c
}

#[test]
fn f10a_threshold_cases() {
    let now = 10_000_000;
    let t = 1_000_000;
    for (off, corrected) in [(4999, false), (5000, false), (5001, true), (-5001, true)] {
        let r = with_offset(off).correct(t, now);
        assert_eq!(r.t_ms != t, corrected, "offset {off}");
        assert_eq!(r.clock_skew, corrected, "offset {off}");
        if corrected {
            assert_eq!(r.t_ms, t + off);
        }
    }
}

#[test]
fn worked_example_section_3_2() {
    let helper_now = 12 * 3_600_000;
    let recv = helper_now + 8000;
    let mut c = ClockOffset::new();
    c.record(recv, helper_now);
    assert_eq!(c.offset_ms(), Some(8000));
    let last = helper_now - 10_000; // 11:59:50.000
    let r = c.correct(last, recv);
    assert_eq!(r.t_ms, helper_now - 2000); // 11:59:58.000
    assert!(r.clock_skew);
    assert_eq!(r.t_ms - 8000, last);
}

fn median_of(samples: &[i64]) -> Option<i64> {
    let mut c = ClockOffset::new();
    for s in samples {
        c.record(1000 + s, 1000);
    }
    c.offset_ms()
}

#[test]
fn median_of_last_five() {
    assert_eq!(median_of(&[7000, 8000, 8000, 9000, 40000]), Some(8000));
    assert_eq!(median_of(&[8000, 8000, 8000, 40000, 40000]), Some(8000));
    // 6th sample evicts the oldest (1).
    assert_eq!(median_of(&[1, 10, 20, 30, 40, 50]), Some(30));
}

#[test]
fn f10b_future_time_clamped_to_now() {
    let now = 5_000_000;
    let c = with_offset(0);
    let r = c.correct(now + 1000, now);
    assert_eq!(r.t_ms, now);
    assert!(r.clock_skew);
    let r = c.correct(now, now);
    assert!(!r.clock_skew);
}
