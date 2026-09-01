//! Deterministic abort points for the crash-durability acceptance test
//! (M4 criterion 2), compiled only under the `crash-points` feature.
//!
//! # Why this exists rather than an external kill
//!
//! Criterion 2 asks what survives the POS dying between confirm and
//! deduction. Killing the process from outside at a guessed moment tests that
//! — sometimes. The kill lands wherever it lands, so the test passes on runs
//! that never reached the interesting instruction and fails intermittently on
//! the ones that do. **A flaky durability test gets disabled, which is worse
//! than not having one at all.**
//!
//! An abort point inside the transaction fires at an exact instruction on
//! every run, with no sleeps and no timing assumptions.
//!
//! # Why `process::abort`
//!
//! It models what is under test. Destructors do not run, so the `Drop` that
//! would seal the database never fires; nothing is flushed; the SQLite
//! connection is never closed; the `-wal` and `-shm` files and the unclean
//! marker are left exactly as a killed process leaves them. A returned error
//! or a panic would unwind and run the very cleanup whose absence is the
//! point.
//!
//! # What this proves, and what it does not
//!
//! It proves the WAL and the transaction boundary survive PROCESS death. It
//! does NOT prove the release binary makes no non-transactional write outside
//! the gated path, and it does not exercise OS page-cache loss — a machine
//! losing power is a different failure mode from a process dying, and neither
//! substitutes for the other. Hard power-cut recovery is part of the parked
//! bare-4GB validation (ADR-013); it has a home and needs no new decision.

/// Between the order being stamped CONFIRMED (with its outbox row) and the
/// stock deduction that rides in the same transaction — the exact window
/// criterion 2 names.
pub const AFTER_CONFIRM_BEFORE_DEDUCT: &str = "after_confirm_before_deduct";

/// Between the `goods_receipt_note` (with its lines and gaps) being written
/// and the `PURCHASE` `stock_ledger_entry` rows that ride in the SAME
/// transaction -- the exact window M5 acceptance criterion 2 names.
///
/// Criterion 2 is judged against the crash, not the API: the receipt and the
/// ledger must AGREE on reopen. Because both are inside one transaction,
/// agreeing means neither is there.
pub const AFTER_GRN_BEFORE_LEDGER: &str = "after_grn_before_ledger";

/// Fires AFTER the goods-receipt transaction COMMITS and BEFORE the database
/// is sealed -- the positive control criterion 2 needs, and the mirror of
/// [`AFTER_GRN_BEFORE_LEDGER`].
///
/// Without it, criterion 2's crash run reads "0 receipts, 0 ledger rows", and
/// that is indistinguishable from a receipt path that silently writes nothing.
/// The absence only means something once the SAME reopen can be shown to find
/// the rows when they were in fact written.
///
/// It must fire after the commit, not before it. An abort inside the
/// transaction rolls everything back, so a reopen finds nothing either way and
/// the control proves as little as no control at all. It must also fire before
/// the seal: a clean exit seals and deletes the decrypted file, and an
/// encrypted database cannot be read by an independent reopen. Aborting in this
/// window is the only state in which committed receipt and ledger rows are on
/// disk and readable.
pub const AFTER_LEDGER_BEFORE_COMMIT: &str = "after_ledger_before_commit";

/// The environment variable naming the point to abort at. Absent (the normal
/// case, including every test that is not about crashing) means no point
/// fires.
#[cfg_attr(not(feature = "crash-points"), allow(dead_code))]
pub const CRASH_POINT_ENV: &str = "HOLLER_CRASH_POINT";

/// Aborts the process if `HOLLER_CRASH_POINT` names this point.
///
/// Read from the environment on each call rather than cached: the cost is a
/// `getenv` on a path that already does file I/O, and a cache would make the
/// point's behaviour depend on when it was first reached.
#[cfg(feature = "crash-points")]
pub(crate) fn maybe_abort(point: &str) {
    if std::env::var(CRASH_POINT_ENV).is_ok_and(|v| v == point) {
        eprintln!("crash-point: aborting at {point}");
        std::process::abort();
    }
}

/// The release shape: nothing. An empty `#[inline(always)]` function so the
/// call sites read the same in both builds and generate no code in this one.
#[cfg(not(feature = "crash-points"))]
#[inline(always)]
pub(crate) fn maybe_abort(_point: &str) {}
