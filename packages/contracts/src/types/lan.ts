// KDS LAN transport messages — added at 0.3.0 (ADR-014, Milestone 2).
//
// This is NOT a sync contract. Nothing here crosses the edge→cloud boundary,
// carries a SyncEnvelope, or touches AGGREGATE_AUTHORITY. It is the wire format
// for one hop: the edge node pushing kitchen state to KDS screens over the
// outlet LAN, so a ticket lands on the pass in well under the 250ms target in
// docs/spec/kitchen.md §Performance target.
//
// It is frozen in contracts rather than hand-rolled at each end because that
// hop has two implementations in two languages — a Rust server in edge/device
// and a TypeScript client in apps/kds — and nothing else would keep them
// agreeing. The Rust side is hand-mirrored (no Rust binding yet, ADR-011
// addendum), so scripts/check-event-type-drift.mjs scans edge/device too.
//
// The edge is authoritative for KOT state (§50.1). A KDS screen sends status
// intent and renders what it is told; it never becomes a second writer.
//
// ---------------------------------------------------------------------------
// TRANSPORT (added 0.3.1, ADR-015)
// ---------------------------------------------------------------------------
// Frozen because two faithful, independent implementations of the message
// shapes below — a Rust server in edge/device and a TypeScript client in
// apps/kds — failed to connect. This file pinned payloads and said nothing
// about the handshake, and a contract that does not pin the interface does not
// pin the interface.
//
//   Endpoint   ws://<edge-lan-host>:<port>/kds
//   Framing    one JSON text frame per message. No sub-protocol negotiation,
//              no outer envelope, no handshake message beyond the socket
//              opening. The first frame the server sends is always a snapshot.
//   Params     outlet_id     required, UUID
//              device_id     required, UUID — see IDENTITY vs AUTHENTICATION
//              station       optional, station code; absent = all stations
//              device_token  optional today, reserved — see below
//   Rejection  HTTP 400 when a required param is missing or empty.
//
// IDENTITY vs AUTHENTICATION — do not conflate these, and do not read the
// current implementation as something this contract blesses:
//
//   `device_id` IDENTIFIES a screen. It does NOT authenticate one. It is a
//   UUID, not a secret, and today it travels in a query string that lands in
//   proxy and access logs. The server currently accepts any device_id matching
//   a registered row, so anyone who reaches the port with a captured id can
//   drive ticket transitions. That is a known unclosed gap, tracked under
//   Device enrollment in docs/backlog-m2.md, and it blocks any pilot
//   deployment.
//
//   `device_token` is the authenticator. It is reserved here — optional and
//   unverified — so that enrollment lands as a BEHAVIOUR change rather than a
//   shape change: clients may send it today and servers may ignore it, and the
//   day verification turns on, only the server's strictness changes. Because
//   the parameter is already part of the frozen handshake, that transition
//   needs a minor bump and an ADR note, not 0.4.0 and not a client rewrite.
//
//   When verification does turn on, `device_token` MUST move out of the query
//   string — an Authorization header, or a first-frame auth message — for the
//   logging reason named above. A secret in a query string is a secret in a
//   log file. Query-string carriage is acceptable only while the value is
//   unverified and therefore worthless.
//
// Still NOT sync: nothing here carries a SyncEnvelope or touches
// AGGREGATE_AUTHORITY. This is one LAN hop.

import { z } from "zod";
import { KotSchema } from "./kot";
import { KotStatusSchema } from "./kot";

// Edge → KDS.
export const KdsLanMessageSchema = z.discriminatedUnion("type", [
  // Full current state, sent on connect and on reconnect. A KDS screen that
  // has been unplugged for an hour must not have to replay a message backlog
  // to become correct, so resynchronisation is a snapshot, never a diff.
  z.object({
    type: z.literal("snapshot"),
    outlet_id: z.string().uuid(),
    sent_at: z.string().datetime(),
    kots: z.array(KotSchema),
  }),
  // A ticket was created or changed. Carries the whole KOT for the same reason
  // the snapshot does: a client that missed the previous message still ends up
  // correct, because the message is self-describing rather than a delta.
  z.object({
    type: z.literal("kot_upserted"),
    outlet_id: z.string().uuid(),
    sent_at: z.string().datetime(),
    kot: KotSchema,
  }),
  // A ticket left the active set (SERVED or CANCELLED). The id alone is enough
  // — the client is dropping the card, not rendering it.
  z.object({
    type: z.literal("kot_removed"),
    outlet_id: z.string().uuid(),
    sent_at: z.string().datetime(),
    kot_id: z.string().uuid(),
  }),
  // Liveness. A KDS screen showing stale tickets and a KDS screen showing
  // nothing look identical to a cook, so silence must be detectable: missing
  // heartbeats are what drive the client's disconnected banner.
  z.object({
    type: z.literal("heartbeat"),
    outlet_id: z.string().uuid(),
    sent_at: z.string().datetime(),
  }),
]);
export type KdsLanMessage = z.infer<typeof KdsLanMessageSchema>;

// KDS → edge. Intent only: the cook asks for a transition, the edge validates
// it against the KOT state machine and answers with a kot_upserted. The screen
// never writes state directly.
export const KdsLanCommandSchema = z.object({
  type: z.literal("set_kot_status"),
  kot_id: z.string().uuid(),
  status: KotStatusSchema,
  // Which screen asked, for the audit trail. The edge does not trust this for
  // authorization — device identity comes from the connection, not the payload.
  device_id: z.string().uuid(),
  requested_at: z.string().datetime(),
});
export type KdsLanCommand = z.infer<typeof KdsLanCommandSchema>;
