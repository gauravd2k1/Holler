//! Edge LAN realtime server (Milestone 2, ADR-014 §6): pushes kitchen state
//! from the outlet edge node to KDS screens over the LAN and accepts
//! `set_kot_status` intent back, validated against the KOT state machine in
//! `holler-edge-database`.
//!
//! The edge is the single authority for KOT state (§50.1) — a KDS screen
//! sends intent, this crate validates and answers; it never writes state
//! directly.

pub mod auth;
pub mod contract;
mod error;
pub mod hub;
pub mod server;

pub use auth::{CloudConfigOracleVerifier, DeviceTokenVerifier};
pub use error::{DeviceError, DeviceResult};
pub use hub::Hub;
pub use server::{start, LanServerHandle, DEFAULT_HEARTBEAT_INTERVAL};

#[cfg(test)]
mod tests;
