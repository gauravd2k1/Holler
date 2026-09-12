# Pilot readiness — everything that must be looked at before the first outlet runs

**Compiled:** 2026-09-11, at the M6 Phase C boundary
**Source:** `docs/backlog.md` (every row whose trigger names a pilot), the Phase A
carries, the Phase B carries, and `docs/adr/ADR-025-table-ordering-device.md`.

**This is a LIST, not a plan.** No new work was invented while writing it, nothing
was re-scoped, and no item was closed on the strength of reading it. Where a row
has been overtaken by work that has since landed, it is marked **RESOLVED** with
what resolved it, because a list that still names fixed things trains its reader
to skim.

**"Blocks pilot"** means: an outlet running this build in a real restaurant would
lose data, lose money, expose credentials, or be unable to start — not "would be
better with it fixed".

**Size** is a rough order of magnitude, given so the list can be triaged, not
committed to: **S** ≈ under a day, **M** ≈ a few days, **L** ≈ a week or more.
Sizes are estimates from reading the entries, not from planning the work.

---

## A. The Phase A carries — what A4, A6 and A7 actually are

Phase A closed with **five of seven landed (A1, A1b, A2, A3, A5) and three
carried**. The kickoff note says only "carried", which is why they are spelled
out here.

### A4 — `Offline` conflates four distinct states · **Blocks pilot: NO** · **Size: S**

`StopReason::Offline` is returned for four different situations: **no listener on
the port**, **a listener that refused the connection**, **a stale pooled socket**,
and **a host that answered with an error**. An operator and a log reader cannot
tell a shop whose WAN is down from a backend that is up and rejecting everything,
and those two need opposite responses — wait, versus call someone.

The transport half landed at `262e03a`. **What is carried is the reporting half
only**, and it must reuse the three-probe fail-closed logic already in
`scripts/check-cloud-unreachable.ps1` rather than growing a second, weaker
version. It degrades diagnosis rather than losing data, which is why Phase A
closed without it.

### A6 — no exit path runs the shutdown drain or seals the database · **Blocks pilot: YES** · **Size: M**

Two observations that turned out to be one.

**(a)** The drain reports through `eprintln` (`state.rs`) and those lines have
been seen in some runs and not others. The work is to establish **which
build/attach state loses them** — a different fix from adding a log line.

**(b)** No exit path on this build fires `RunEvent::Exit`. Closing the window
under `tauri dev` leaves `holler-pos.exe` alive with the pump still ticking and
the database open; `Ctrl+C` from the launching terminal — the documented correct
way — terminates the process but **still does not seal**. Observed 2026-09-05,
again 2026-09-07, and again 2026-09-10, when `edge.db.enc` carried the time the
app *opened* while an evening's trading sat in the plaintext file beside it.
Observed once more while writing this report: a plaintext `edge.db` and its
`-wal` sitting alongside the `.enc` at an identical mtime.

**THE PLAINTEXT LEFTOVER IS NOT A RECOVERY ROUTE, and must never be offered as
one** (operator's ruling, 2026-09-12, after an agent suggested reading it back
when a key was feared lost). It is readable with no key — that is the
data-loss/credential-exposure bug itself, not a feature to lean on. **If the key
is lost, the answer is a reset.** Treating the leftover as a fallback is how a
bug becomes a workflow and then stops being fixed.

**Why this blocks a pilot:** it is the mechanism behind the credential-exposure
row in §B, and it is why no trustworthy backup can be taken before a risky
migration (§B, rebuild-class migrations). It is also why M6 C4's falsifier could
not isolate an "abnormal" exit — there is no normal one.

### A6b — `seal_file` writes IN PLACE, so an interrupted seal can lose the outlet's day · **Blocks pilot: YES** · **Size: S**

**Grouped with A6 and not filed as a footnote, because it is the same family:
the outlet's own data, lost on a path nobody watches.** A6 is "the exit never
seals". This is "the seal itself is not atomic". Together they are the two ways
a trading day reaches the end of service and is not in the sealed file.

`crypto::seal_file` encrypts and writes the result **directly over
`edge.db.enc`**. There is no temp file and no rename, so the window between the
first byte and the last is a window in which the sealed database is neither the
old one nor the new one. A crash, a power cut, a kill, or a full disk inside
that window leaves a **truncated or half-written sealed file** — and the key
verification that guards the other direction (T25) cannot help, because a
corrupt ciphertext fails to authenticate and the operator is left with a
database that opens under no key at all.

**This is not hypothetical.** On 2026-09-12 the operator's `edge.db` held
1,392,640 bytes of ciphertext — exactly `edge.db.enc` minus AES-GCM's 12-byte
nonce and 16-byte tag — a sealed payload sitting at the plaintext path. Crash
recovery now quarantines such a file rather than dying on it (`10a6e22`), so
the symptom is handled. **How the bytes got there is still unexplained**, and
an in-place writer that can mislabel a file in one direction can truncate one
in the other.

**The fix, named:**

1. Encrypt to `edge.db.enc.tmp` in the same directory (same volume, so the
   rename is atomic).
2. **`fsync` the temp file**, then `fsync` the directory. Without both, a
   rename can reach the disk before the bytes it names — the classic
   crash-consistency hole, and the reason "write then rename" alone is not
   enough.
3. `fs::rename` over the target. On Windows and on POSIX this replaces
   atomically: a reader sees either the whole old file or the whole new one,
   never a mixture.
4. Only then wipe the plaintext.

Add a test that kills the process between steps 1 and 3 — or, if that is
impractical in-process, one that leaves a stale `.tmp` behind and proves the
next open ignores it and still reads the sealed file. **A seal that cannot be
half-applied is the property; a test that only checks the happy path proves
nothing about it.**

### A7 — 78 outbox rows have no edge route and can never be sent · **Blocks pilot: YES** · **Size: M**

`edge/sync/src/route.rs` maps only `order` and `table_session`. Every other
aggregate's rows are reported `unrouted_skipped` and sit pending forever; the
drain counts them and nothing can send them.

Measured on the live edge database 2026-09-07: **78 pending rows with no route —
55 `kot`, 22 `stock_count`, 1 `invoice`** — alongside 24 pending `order` rows
that do have one. The cloud ingest routes already exist, so this is an edge
resolver and envelope job covering `kot`, `invoice`, `payment`, `cash_shift`,
`stock_count` and item availability.

**This is the largest single carry out of Phase A**, the reason the pending count
never reaches zero however healthy the drain looks, and a prerequisite for the
table tab (ADR-025) and for any aggregate beyond `order` replaying at all. An
outlet running a pilot today would keep every invoice and every payment on one
disk indefinitely.

---

## A-bis. Key management — where the edge database key lives, and who can change it

**Blocks pilot: YES. Size: M.** Added 2026-09-11, after the dev machine's key
was found to be 32 repeated bytes.

`HOLLER_DB_KEY_HEX` is the AES key for the edge SQLite database at rest
(ADR-011). It protects the cached Argon2id password and PIN hashes that let
staff log in with the uplink down, the `device_credential_cache` verifiers the
KDS and captain page authenticate against, and every order, invoice and payment
the till has taken.

**Today it lives in `apps/pos/.env.dev`: a plaintext file that
`scripts/dev-bootstrap.ps1` rewrites in full on every run.** That is a
development arrangement standing in for a production one, and nothing about it
is suitable for an outlet.

### The wrong-key path destroys the database rather than refusing — CORRECTED 2026-09-11

This entry originally repeated the scripts' own warning, that a wrong key
"opens a different, empty database, not an error". **Reading the code showed
that is wrong in the reassuring direction, and the truth is worse.**

`Db::open` (`edge/database/src/lib.rs:104`) runs `crypto::recover_crash_leftovers`
**before** `crypto::open_file`. With no plaintext leftover the behaviour is
already correct: `open_file` fails to decrypt and the POS refuses to start,
pinned by `crypto.rs`'s `open_with_wrong_key_fails`.

But when a plaintext leftover is present, recovery opens it — **no key is needed,
it is plaintext** — and then calls `seal_file(plaintext, sealed, key)`, resealing
it over `edge.db.enc` **under whatever key was supplied**, with nothing checking
that the key matches the file it is overwriting. `open_file` then succeeds,
because the file was just encrypted with the key in hand.

So a wrong key does not open an empty database. **It overwrites the real one**,
and the correct key then opens nothing, because nothing encrypted under it
remains.

**Gap A6 makes this the normal case, not an edge case**: no exit path on this
build seals the database, so a plaintext `edge.db` is left beside the `.enc`
after every run.

Fixed in the demo build — recovery now proves the key opens the existing sealed
file before it is allowed to reseal over it. The entry stays on this list
because the surrounding questions remain open, and because the wrong wording
survived in two scripts and one register until someone read the code.

Five questions, none of which has an answer yet:

- **Generation.** Who mints the key for an outlet, on what machine, and how is
  it proved random? The interim guards added on 2026-09-11 reject a key that
  fails an entropy heuristic, which catches a hand-typed placeholder and nothing
  subtler.
  **The mint command both scripts print uses `Get-Random`, which is
  `System.Random` — a deterministic PRNG seeded from the clock, not a CSPRNG.**
  For a key protecting cached credential hashes and an outlet's trading history
  that is the wrong generator: its output is predictable to anyone who can
  bracket the time the key was minted. **RESOLVED in the demo build**
  (`2c8093d`): all three call sites now use
  `[System.Security.Cryptography.RandomNumberGenerator]::Create()`, verified on
  this machine's PowerShell 5.1 — preferred over `RNGCryptoServiceProvider`,
  which is obsolete on newer runtimes.
  Note what the entropy heuristic could not have done here: a `Get-Random` key
  looks perfectly random to it, because the weakness was in how the value was
  **produced** and not in how it is distributed. Two guards on one value,
  neither able to see the other's failure — which is why "who mints it, on what
  machine" stays an open question for a pilot even though the command is now
  correct.
- **Storage.** A plaintext `.env` beside the binary is the key sitting next to
  the lock. Windows DPAPI, the Credential Manager, or a TPM-sealed blob are the
  obvious candidates on ADR-013's hardware; each needs a decision and a fallback
  for a machine that has none.
- **Rotation.** **A different key does not error — it silently opens a
  different, empty database.** So a mistaken rotation presents as a working till
  with no history, indistinguishable from a fresh install. Rotation therefore
  needs a re-encrypt path, not a key swap, and the interim `-RotateKey` flag is
  a guard against accident, not a rotation mechanism.
- **Backup and recovery.** ADR-011 forbids the database ever being copied
  anywhere unencrypted, so a backup is useless without its key and the key
  cannot live with the backup. **Losing the key loses the outlet's trading
  history** — and gap A6 already means the sealed file is only as current as the
  last successful seal.
- **Machine replacement.** A till dies mid-service and is swapped. What does the
  new machine need, who holds it, and how long does the outlet wait?

**Trigger: before the first pilot.** Related: the 2026-09-11 retro entry on a
deny rule protecting a file rather than a secret, and gap A6 (no exit path
seals the database).

---

## B. Carried by a pilot trigger, from `docs/backlog.md`

Grouped by what they threaten. Titles are abbreviated; the backlog row is the
authority.

### B1. Data leaves the till, or does not

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **A7 — aggregates with no edge route** | **YES** | M | §A above |
| **A 500 on a replayed row wedges the outbox forever** | — | — | **RESOLVED** by A1/A1b/A2/A3; M6 C7 closed 2026-09-07 and M6 C3 2026-09-11 |
| **No periodic sync pump while the till is open** | — | — | **RESOLVED** by A5; M6 C4 observed 2026-09-11 |
| **`edge/sync` has no host — nothing that ships calls it** | — | — | **RESOLVED**: the A5 loop drives both the pump and `pull_and_apply_config` (contracts 0.7.0) |
| **The shutdown drain is the only outbound path** | — | — | **SUPERSEDED** by A5 for data loss; the exit-path half survives as A6 |
| **A6 — no exit path drains or seals** | **YES** | M | §A above |
| **Rebuild-class migration can leave the edge database mid-rebuild, and the `.enc` backup cannot be trusted** | **YES** | M | Two halves; the backup half is a prerequisite for the other. Filed 2026-09-11 |
| **Three block-and-budget mechanisms for one concept** | NO | M | Drift risk, not loss. Trigger is also "a fourth stream needs a budget" |

### B2. The cloud's copy is wrong

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **Nothing checks a line's `variant_id` against the cloud before queueing; failure mode is a silently divergent copy** | **YES** | M | Cause of the row below. Lands with the config-push work |
| **The cloud's copy of an order stops tracking the till after create** | **YES** | — | Same fix as above; cause identified 2026-09-10 |
| **The inventory config push has never moved a row** | **YES** | M | Never demonstrated for any catalogue |
| **The cloud menu seed is a token (2 rows) and the edge's is real (43)** | **YES** | S | Same family |
| **The cloud devseed inserts `menu_item` with no `hsn_sac`** | **YES** | S | An invoice cannot legally issue without it |
| **A menu item deleted in the cloud is never removed from an edge** | NO | M | Apply upserts and does not prune |

### B3. Security and access

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **No clean exit path, so the plaintext edge database with credential hashes lands on disk every shutdown** | **YES** | M | Same root cause as A6 |
| **Device enrollment flow — no operator-facing flow exists** | **YES** | L | Hard trigger: *any* pilot deployment. No device LIST route; the 409 body carries no id |
| **Split `outlet.manage` — it has become a de-facto admin role** | **YES** | M | One grant gates table config, GSTIN writes and hardware enrollment. Enrollment sits behind it |
| **LAN security gate review for a public-facing device** | NO | M | Trigger is the first tab enrolled, not this pilot |
| **Repository is public** | NO | — | **DECIDED 2026-08-31: stay public.** Listed so it is not re-raised |

### B4. Money and reporting

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **Nothing distinguishes tax-inclusive from tax-exclusive purchase price at entry** | NO | M | Mitigation is a field label; the real fix needs input-tax-credit handling |
| **Cost is a lifetime cumulative purchase-weighted average, not WAC of stock on hand** | NO | M | Only half was ever decided; food costing is a headline claim |

### B5. What the operator sees

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **The sync banner is unreadable on a till** | NO | S | The only surface a rejected row is ever shown on |
| **The banner prints `aggregate_id`, so one order appears once per outbox row** | NO | S | Land with the row above |
| **The banner covers the POS top bar** | NO | S | Land with the two rows above — one pass |
| **The pump makes the till sluggish while the cloud is down** | NO | S–M | Against ADR-013's own promise. Severity at the shipped 60s interval is unmeasured |
| **The Orders screen renders raw UTC** | NO | S | Read as a clock fault by the operator mid-run |
| **Four M1/M2 POS ordering defects** | NO | M | Filed with an M6 trigger, listed here as operator-facing |

### B6. Tooling and environment

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **`dev-up.ps1` runs the bootstrap before starting the backend, so a cold stack comes up with sync disabled** | NO | S | Also triggers before the next sync-dependent acceptance run |
| **A wall-clock assertion in `stale_connection.rs:160` fails under load** | NO | S | A flaky suite is how a real regression gets waved through |

---

## C. The Phase B carries

Phase B closed with **three surfaces built and two carried** — not all five.

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **Admin: PURCHASE ORDERS surface** | NO | M | Deferred because no M6 criterion touched it |
| **Admin: STAFF AND PERMISSIONS surface** | **YES** | M | An outlet cannot create or change its own staff without it; interacts with the `outlet.manage` split |
| **`apps/admin` mints ids with `crypto.randomUUID()` (UUIDv4), against §74** | NO | S | Every supplier and pack size created in the console |
| **Admin supplier form collects no GSTIN; pack-size table shows a raw UUID** | NO | S | — |

---

## C2. The menu the client actually sells

Added by the demo menu swap (2026-09-12), when the invented dev menu was
replaced by the client's own card. All three are contract shapes, not bugs.

| Item | Blocks pilot | Size | Note |
|---|---|---|---|
| **`tax_rule.component` cannot express VAT — alcohol is billed at a ZERO RATE** | **YES** | M | Contracts change, both stores plus engine. Every bar line the outlet sells: the VAT it owes is not computed, not printed and not collected. Added by the demo menu swap, 2026-09-12 |
| **`menu_item` has no `is_veg`** | **YES** | S | FSSAI requires the veg/non-veg marker on a menu. Additive contracts bump plus the four surfaces that render an item |
| **`menu_item` has no `description`** | NO | S | Ships with `is_veg`; a card of bare names is sellable, just poorer |

---

## D. Summary

**Blocks a pilot — 14 items:** A6, A7, the rebuild/backup pair, the four
cloud-copy and config-push rows (`variant_id` check, order copy, inventory push,
menu seed), the cloud `hsn_sac` seed, the plaintext database on shutdown, device
enrollment, the `outlet.manage` split, the admin staff surface, the two menu
shapes added on 2026-09-12 (**VAT is inexpressible, so alcohol bills at a zero
rate**, and **`menu_item` has no `is_veg`** where FSSAI requires the marker),
and **A6b, the non-atomic seal**.

They are not thirteen independent pieces of work. **Three roots account for eight
of them:**

1. **No exit path drains or seals, and the seal is not atomic** — A6, A6b, the
   plaintext database, and the untrustworthy backup that blocks safe
   migrations. A6 loses the day by never sealing; A6b can lose it by sealing
   halfway.
2. **The config push has never moved a row for any catalogue** — inventory, menu,
   `hsn_sac`, the `variant_id` check, and the divergent cloud copy.
3. **Enrollment has no operator-facing flow** — the flow itself, the missing
   device LIST route, and the `outlet.manage` split it sits behind.

A7 and the admin staff surface stand alone.

**Does not block, but is visible to an operator every shift:** the three banner
rows, the UTC column and the pump sluggishness — five items, all small, all on
the two screens an outlet looks at most.

**Explicitly decided and not to be re-raised:** the repository stays public
(2026-08-31). **Explicitly parked on hardware:** ESC/POS on paper and the bare
4GB Windows 10 VM run, both in `CLAUDE.md`.

**Not on this list by design:** ADR-025's table-ordering work, whose trigger is
*after* the first pilot runs on `STAFF_ONLY`, and M6 C2, parked behind platform
sandbox access.
