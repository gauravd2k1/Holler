//! Tauri command handlers — thin IPC boundary only (CLAUDE.md: "business
//! logic outside UI components"). Each handler locks the shared `Db`,
//! delegates arithmetic/validation to `crate::domain`, and maps
//! `holler_edge_database`/`crate::domain` errors to `crate::error::AppError`.

pub mod auth;
pub mod billing;
pub mod inventory;
pub mod kitchen;
pub mod menu;
pub mod orders;
pub mod tables;
