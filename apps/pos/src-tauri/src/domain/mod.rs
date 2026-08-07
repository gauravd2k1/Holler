//! Business logic modules. Nothing here depends on `tauri` or `rusqlite`
//! directly — command handlers in `crate::commands` are the thin boundary
//! that wires this to storage and IPC (CLAUDE.md coding rules).

pub mod order;
