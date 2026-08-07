//! Edge-local id and timestamp generation. The edge is authoritative for
//! operational transactions (sync.md §50.1, §74): order/order_item/outbox
//! ids are minted here, never assigned by the cloud, and never blocked on
//! network availability.

use uuid::Uuid;

/// A new UUIDv7 as a lowercase hyphenated string — k-sortable by creation
/// time, matching the `id` column convention documented across
/// `packages/contracts/sqlite/*.sql`.
pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

/// Current UTC instant as an ISO8601 string with a literal `Z` offset,
/// matching the `TEXT` timestamp columns in the frozen SQLite schema and the
/// `z.string().datetime()` Zod validators in `packages/contracts`.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}
