//! M6 A5 falsification: the periodic sync pump.
//!
//! The gap this closes: `drain_outbox` had exactly two callers — startup
//! (`state.rs`) and `RunEvent::Exit` (`lib.rs`) — so a till that exited
//! abnormally never drained, and the day's orders waited for whenever
//! somebody next launched the application. M5 ended with 120 rows pending
//! for exactly this reason, and the 2026-09-05 C7 run charged its four
//! rejected rows only twice in total because attempts can only accrue when
//! the application starts or stops.
//!
//! Three claims are load-bearing here and none is self-evident, so each is
//! falsified rather than asserted:
//!
//! 1. **The timer actually fires, more than once, without anyone touching
//!    the application.** A pump that never ticks is indistinguishable from
//!    the pre-A5 build, and green.
//! 2. **The loop stops PROMPTLY when the flag is set.** The interval is
//!    served as short slices for this reason; a loop that finishes its full
//!    sleep before noticing would hold up shutdown for a minute, which is
//!    the "till that will not close" ADR-020 spent a budget avoiding.
//! 3. **The loop does not tick on the way out.** A pump that fires once more
//!    after the stop flag is set is a drain running against a database the
//!    exit path is sealing, and its failure mode is the silent one.
//!
//! These test WHEN, not WHAT: the drain's own behaviour — classification,
//! per-aggregate blocking, the retry budget, the deadline — is unchanged by
//! A5 and is covered by `adr020_outbox_drain.rs` and the sync crate. A5 adds
//! a caller and nothing else, which is the point.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use holler_pos_lib::state::{run_periodic_drain_loop, DEFAULT_PERIODIC_DRAIN_INTERVAL};

/// Joins a pump thread, FAILING on a deadline rather than blocking forever.
///
/// `JoinHandle::join` has no timeout, so a loop that never notices its stop
/// flag would hang the test binary instead of failing it — and a hung suite
/// is worse than a red one: it wedges CI, reports nothing, and is
/// indistinguishable from a slow machine. Measured: with both stop checks
/// removed from `run_periodic_drain_loop`, a plain `join()` here never
/// returns. This makes that outcome a failure with a message.
fn join_within(handle: std::thread::JoinHandle<()>, limit: Duration) {
    let deadline = Instant::now() + limit;
    while !handle.is_finished() {
        assert!(
            Instant::now() < deadline,
            "the pump thread did not stop within {limit:?} of its flag being set"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    handle.join().expect("the pump thread must not panic");
}

/// Claim 1: it fires repeatedly, on its own.
///
/// FALSIFIED FIRST by giving `run_periodic_drain_loop` a body that returns
/// before its first tick: the count stays 0 and this fails. That is the
/// pre-A5 build's behaviour expressed as a test — nothing drains unless a
/// human starts or stops the application.
#[test]
fn the_pump_ticks_repeatedly_without_anyone_touching_the_application() {
    let stop = Arc::new(AtomicBool::new(false));
    let ticks = Arc::new(AtomicUsize::new(0));

    let loop_stop = Arc::clone(&stop);
    let loop_ticks = Arc::clone(&ticks);
    let handle = std::thread::spawn(move || {
        run_periodic_drain_loop(&loop_stop, Duration::from_millis(120), || {
            loop_ticks.fetch_add(1, Ordering::SeqCst);
        });
    });

    // Three intervals plus slack. Generous on purpose: this asserts that the
    // timer fires at all and keeps firing, never a precise cadence — a
    // wall-clock equality assertion on a busy CI box is a flake, and this
    // repository has already paid for one of those (`stale_connection.rs`).
    let deadline = Instant::now() + Duration::from_secs(5);
    while ticks.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.store(true, Ordering::SeqCst);
    join_within(handle, Duration::from_secs(5));

    let observed = ticks.load(Ordering::SeqCst);
    assert!(
        observed >= 3,
        "the periodic pump must fire repeatedly on its own; observed {observed} tick(s)"
    );
}

/// Claim 2: setting the flag stops it promptly, not at the end of the
/// current interval.
///
/// The interval here is far longer than the assertion window, so a loop that
/// sleeps the whole interval before re-checking cannot pass: this fails
/// outright if the sleep is not sliced.
#[test]
fn the_pump_stops_promptly_rather_than_finishing_its_interval() {
    let stop = Arc::new(AtomicBool::new(false));
    let ticks = Arc::new(AtomicUsize::new(0));

    let loop_stop = Arc::clone(&stop);
    let loop_ticks = Arc::clone(&ticks);
    let handle = std::thread::spawn(move || {
        run_periodic_drain_loop(&loop_stop, Duration::from_secs(60), || {
            loop_ticks.fetch_add(1, Ordering::SeqCst);
        });
    });

    std::thread::sleep(Duration::from_millis(150));
    let asked_at = Instant::now();
    stop.store(true, Ordering::SeqCst);
    join_within(handle, Duration::from_secs(5));
    let took = asked_at.elapsed();

    assert!(
        took < Duration::from_secs(5),
        "the pump must notice the stop flag inside its interval, not at the end of it; took {took:?}"
    );
    assert_eq!(
        ticks.load(Ordering::SeqCst),
        0,
        "a 60s interval cannot have produced a tick in 150ms"
    );
}

/// Claim 3: it does not fire on the way out, and it returns.
///
/// The flag is set before the loop ever starts. In production this is the
/// ordering that matters: the exit path sets the flag and then seals the
/// database, so a tick afterwards drains a closed connection.
///
/// WHAT THIS TEST ACTUALLY FALSIFIES, stated exactly, because the honest
/// answer is narrower than the test name suggests. `run_periodic_drain_loop`
/// checks the flag twice — once inside the wait, once after it — and the two
/// checks cover for each other. Removing EITHER one alone leaves this test
/// green (measured, both directions). Removing BOTH makes the loop run
/// forever, so the failure is a HANG, not an assertion. A falsifier that
/// hangs is indistinguishable from a slow machine and would wedge CI rather
/// than fail it, which is why the loop runs on its own thread here with a
/// join deadline: with both checks gone this fails on the deadline in a few
/// seconds instead of never returning.
///
/// The redundancy is deliberate and is kept — the outer check closes the
/// window between the wait ending and the tick starting — but it is recorded
/// here rather than left for a later reader to discover that neither check
/// appears individually load-bearing.
#[test]
fn a_pump_told_to_stop_before_it_starts_never_ticks_and_returns() {
    let stop = Arc::new(AtomicBool::new(true));
    let ticks = Arc::new(AtomicUsize::new(0));

    let loop_ticks = Arc::clone(&ticks);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        run_periodic_drain_loop(&stop, Duration::from_millis(10), || {
            loop_ticks.fetch_add(1, Ordering::SeqCst);
        });
        // A send failure means the receiver already gave up on the deadline
        // below; the assertion there is the report, so this drops silently.
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("a pump whose stop flag is already set must RETURN, not spin forever");

    assert_eq!(
        ticks.load(Ordering::SeqCst),
        0,
        "the pump must check its stop flag BEFORE its first tick, not after"
    );
}

/// The default cadence is a decision, so it is pinned rather than left to
/// drift, and pinned against the property that matters: it must be longer
/// than the drain's own budget, or two drains can stack up behind each other
/// whenever the cloud is unreachable and every pass runs to its deadline.
#[test]
fn the_default_interval_is_longer_than_one_drain_budget() {
    assert!(
        DEFAULT_PERIODIC_DRAIN_INTERVAL > holler_pos_lib::state::SHUTDOWN_DRAIN_BUDGET,
        "the pump interval ({:?}) must exceed the drain budget ({:?})",
        DEFAULT_PERIODIC_DRAIN_INTERVAL,
        holler_pos_lib::state::SHUTDOWN_DRAIN_BUDGET
    );
}
