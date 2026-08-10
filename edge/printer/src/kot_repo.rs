//! Reads `kot` rows this crate needs for rendering. `edge/database` owns
//! writing `kot` and exposes `list_kots_for_order`, but not a get-by-id — this
//! crate only ever needs one `kot` row at a time (the one it is about to
//! print), so a small local read keeps this crate from needing a change to
//! `edge/database` for a single query. Field layout matches
//! `holler_edge_database::model::Kot` exactly and constructs that same
//! public type, so callers never see two different `Kot` shapes.

use holler_edge_database::model::Kot;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::PrinterResult;

pub fn get_kot_by_id(conn: &Connection, id: &str) -> PrinterResult<Option<Kot>> {
    conn.query_row(
        "SELECT id, order_id, station, sequence, status, items_json, created_by_device_id, created_at, updated_at
         FROM kot WHERE id = ?1",
        params![id],
        |row| {
            Ok(Kot {
                id: row.get(0)?,
                order_id: row.get(1)?,
                station: row.get(2)?,
                sequence: row.get(3)?,
                status: row.get(4)?,
                items_json: row.get(5)?,
                created_by_device_id: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}
