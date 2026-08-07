//! Pure backoff calculation. Kept separate from any sleeping/scheduling —
//! this crate never blocks in a busy loop; the caller (whatever schedules
//! worker ticks, e.g. a Tauri background task) reads this value to decide
//! when to call the worker again.

const BASE_MS: u64 = 1_000;
const MAX_MS: u64 = 5 * 60 * 1_000; // 5 minutes

/// Exponential backoff with a ceiling, given how many attempts an outbox row
/// has already made (post-increment). `attempt_count == 1` (first failure)
/// yields `BASE_MS`; it doubles per additional attempt up to `MAX_MS`.
pub fn backoff_ms(attempt_count: i64) -> u64 {
    if attempt_count <= 0 {
        return 0;
    }
    let shift = (attempt_count - 1).min(20) as u32; // guard against overflow
    BASE_MS.saturating_mul(1u64 << shift).min(MAX_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_attempt_is_base_delay() {
        assert_eq!(backoff_ms(1), BASE_MS);
    }

    #[test]
    fn grows_and_caps() {
        assert_eq!(backoff_ms(2), 2_000);
        assert_eq!(backoff_ms(3), 4_000);
        assert_eq!(backoff_ms(100), MAX_MS);
    }

    #[test]
    fn zero_attempts_has_no_delay() {
        assert_eq!(backoff_ms(0), 0);
    }
}
