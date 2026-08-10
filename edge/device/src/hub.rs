//! Broadcast fan-out from the edge to every connected KDS screen watching one
//! outlet, optionally narrowed to one station (kitchen/tandoor/bar all watch
//! the same outlet but not necessarily the same station).
//!
//! Each subscriber gets its own bounded channel. Publishing uses `try_send`:
//! a slow or dead client fills its own channel and starts dropping messages
//! for itself, but never blocks the publisher or any other subscriber
//! (ADR-014 §6 / Milestone 2 DoD: "a slow or dead client must not block the
//! others"). Because every `KdsLanMessage` is self-describing (whole `Kot`,
//! whole snapshot), a dropped message is recovered by the next one, and
//! worst case by the next reconnect snapshot — never by a delta replay.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;

use crate::contract::{Kot, KdsLanMessage};

/// How many outstanding messages a subscriber may lag by before this hub
/// starts dropping messages for it rather than blocking the publisher.
const SUBSCRIBER_CHANNEL_CAPACITY: usize = 64;

struct Subscriber {
    conn_id: u64,
    /// `None` watches every station at the outlet (an expo/pass screen).
    station: Option<String>,
    sender: SyncSender<KdsLanMessage>,
}

#[derive(Default)]
struct OutletSubscribers {
    subscribers: Vec<Subscriber>,
}

/// Shared, thread-safe fan-out registry. One `Hub` per running
/// [`crate::server::LanServer`], cloned via `Arc` into every connection
/// thread and into whatever else in the process needs to announce a KOT
/// change (the WS command handler in this crate, and — outside this crate —
/// any other write path, e.g. the POS's own send-to-kitchen command).
#[derive(Default)]
pub struct Hub {
    by_outlet: Mutex<HashMap<String, OutletSubscribers>>,
    next_conn_id: AtomicU64,
}

/// A live subscription, returned to a connection so it can later
/// [`Hub::unsubscribe`] itself and read published messages off `receiver`.
pub struct Subscription {
    pub conn_id: u64,
    pub outlet_id: String,
    pub receiver: Receiver<KdsLanMessage>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, outlet_id: &str, station: Option<String>) -> Subscription {
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = sync_channel(SUBSCRIBER_CHANNEL_CAPACITY);
        let mut map = self.by_outlet.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(outlet_id.to_string())
            .or_default()
            .subscribers
            .push(Subscriber {
                conn_id,
                station,
                sender,
            });
        Subscription {
            conn_id,
            outlet_id: outlet_id.to_string(),
            receiver,
        }
    }

    pub fn unsubscribe(&self, outlet_id: &str, conn_id: u64) {
        let mut map = self.by_outlet.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.get_mut(outlet_id) {
            entry.subscribers.retain(|s| s.conn_id != conn_id);
        }
    }

    /// Number of currently registered subscribers for an outlet (any
    /// station). Test/observability helper.
    pub fn subscriber_count(&self, outlet_id: &str) -> usize {
        let map = self.by_outlet.lock().unwrap_or_else(|e| e.into_inner());
        map.get(outlet_id).map(|e| e.subscribers.len()).unwrap_or(0)
    }

    /// Sends `message` to every subscriber of `outlet_id` whose station
    /// filter matches `station_hint`. `station_hint = None` means "relevant
    /// to every station" (heartbeat, snapshot-at-connect, and
    /// `kot_removed`, whose payload deliberately carries no station —
    /// lan.ts — so it goes to everyone at the outlet).
    fn publish(&self, outlet_id: &str, message: KdsLanMessage, station_hint: Option<&str>) {
        let map = self.by_outlet.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = map.get(outlet_id) else {
            return;
        };
        for subscriber in &entry.subscribers {
            let relevant = match (&subscriber.station, station_hint) {
                (None, _) => true,
                (Some(_), None) => true,
                (Some(watched), Some(hint)) => watched == hint,
            };
            if !relevant {
                continue;
            }
            match subscriber.sender.try_send(message.clone()) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    log::warn!(
                        "kds lan: dropping message for slow subscriber conn={} outlet={}",
                        subscriber.conn_id,
                        outlet_id
                    );
                }
            }
        }
    }

    /// A KOT was created or changed. Broadcast to every subscriber at the
    /// outlet watching that KOT's station (or watching every station).
    pub fn notify_kot_upserted(&self, outlet_id: &str, kot: &Kot, sent_at: &str) {
        let station = kot.station.clone();
        let message = KdsLanMessage::KotUpserted {
            outlet_id: outlet_id.to_string(),
            sent_at: sent_at.to_string(),
            kot: kot.clone(),
        };
        self.publish(outlet_id, message, Some(&station));
    }

    /// A KOT left the active set (SERVED/CANCELLED). No station in the
    /// payload (lan.ts), so this reaches every subscriber at the outlet.
    pub fn notify_kot_removed(&self, outlet_id: &str, kot_id: &str, sent_at: &str) {
        let message = KdsLanMessage::KotRemoved {
            outlet_id: outlet_id.to_string(),
            sent_at: sent_at.to_string(),
            kot_id: kot_id.to_string(),
        };
        self.publish(outlet_id, message, None);
    }

    /// Liveness beat, sent to every subscriber at every outlet with at least
    /// one connection.
    pub fn heartbeat_all(&self, sent_at: &str) {
        let outlets: Vec<String> = {
            let map = self.by_outlet.lock().unwrap_or_else(|e| e.into_inner());
            map.keys().cloned().collect()
        };
        for outlet_id in outlets {
            let message = KdsLanMessage::Heartbeat {
                outlet_id: outlet_id.clone(),
                sent_at: sent_at.to_string(),
            };
            self.publish(&outlet_id, message, None);
        }
    }
}
