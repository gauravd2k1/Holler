# The VV sitting — one launch, every open row

**Prepared 2026-09-18 by the autonomous M7 run. NOT RUN.** Every row below is
`OPEN` in `docs/visual-verify.md` and an agent cannot close any of them:
`run-dev.ps1` refuses under `CLAUDECODE`, and a Tauri window launched from a
tool with redirected stdio never appears.

**19 VV rows are open** (VV-001 … VV-020; VV-009 is closed as a FAIL and is
superseded by VV-012/013/014). Three extra rows sit alongside them because they
need the same sitting: **B1-0**, the Chrome observation that unblocks the admin
console track, and **A6-1 / A6-2**, the two exit probes.

**Total estimate: 75–95 minutes**, including one clean reset (~10 min) and the
cloud stop/start in phase 5.

## How to use this file

Work top to bottom. The order exists so that **one launch of the stack covers
everything** and no row destroys the precondition of a later one — the exit
probes are last for exactly that reason.

For each row: do the clicks, decide PASS or FAIL against the two columns, save
the screenshot under the given name, and paste the results back. A row with no
screenshot does not close (`docs/visual-verify.md` rule 2).

Save every screenshot to **`docs/evidence/`** with the filename given.

---

## The binary

| | |
|---|---|
| **Path** | `apps\pos\src-tauri\target\release\holler-pos.exe` |
| **Content-verified?** | **YES** — rebuilt and re-checked 2026-09-18 16:56; see below |
| **How to re-check** | `.\scripts\check-release-binary.ps1` |

**REBUILT DURING THIS RUN, AND RE-VERIFIED.** The binary on disk at the start
of the sitting prep was built **2026-09-17 19:10** and was content-verified
against **that day's** `dist` (entry chunk `index-Cw4hDDCP.js`). It therefore
contained VV-017/018/019 (from `045b68e`) but **NOT VV-020's fix**, which
landed today at `d0af9a8` — so the sitting would have tested the new row
against a binary that does not contain it.

`pnpm exec tauri build` was run. `check-release-binary.ps1` afterwards:

```
  binary : C:/Code/Holler/apps/pos/src-tauri/target/release/holler-pos.exe
  built  : 2026-09-18 16:56:18
    needle: index-B7byvtKn.js
    needle: index-COLAtMd0.css
  OK -- the release binary carries this frontend (2 asset name(s) found inside it).
```

**The entry chunk changed** (`index-Cw4hDDCP.js` -> `index-B7byvtKn.js`), which
is the positive evidence that today's frontend is inside it. Note the check is
deliberately keyed on the PRESENCE of this dist's hashed filenames, never on
the ABSENCE of a `localhost:5173` string — a correct production binary still
embeds the whole `tauri.conf.json`, `devUrl` included, so an absence check
passes the broken binary and proves nothing.

**Before starting, run `.\scripts\check-release-binary.ps1` yourself and read
the line that says which asset names it found inside the binary.** That check
exists because every *existence and identity* check passes on a binary that
cannot draw a window — the 2026-09-15 incident, where the release binary had
been fetching its UI from `localhost:5173` for three sessions.

---

## Phase 0 — the browser, before anything is launched (5 min)

### B1-0 — name the admin console's "Failed to fetch" (5 min)

**This is B1's falsifier.** Until it exists, every fix in that track is a
guess — the track's own design says so. Nothing else in B1 may start.

**What to open:** the admin console in **your normal Chrome** — your profile,
your extensions enabled, HTTPS-First at whatever it is set to. Not Incognito,
not VS Code's Simple Browser. The backend must be up.

**Clicks:**
1. Open the admin console at `http://localhost:5175`.
2. Open DevTools ▸ Console.
3. Paste the snippet at `docs/demo-runbook.md:450` (the same call the sign-in
   form makes) and press Enter.
4. Read what it prints.

**PASS looks like:** it does not matter whether it succeeds or fails — **the
row closes when the cause is NAMED.** Record which of the four it is:

| Result | Cause |
|---|---|
| `OK {...}` with a token | The API is reachable; the failure is in the app, not the transport |
| `ERR TypeError: Failed to fetch` **and** the Network tab shows the request as `(blocked:...)` or never sent | An **extension**, or **HTTPS-First** upgrading the URL |
| A CORS message naming the origin | The **address bar origin** — `127.0.0.1` instead of `localhost` |
| It works here but not in the form | A **stale service worker** on 5175 |

**FAIL looks like:** nothing prints, or the result is ambiguous. Then also try
an Incognito window (extensions off) and record the difference — that single
comparison separates "extension" from everything else.

**Screenshot:** `docs/evidence/B1-0-console.png` — include the Console **and**
the Network tab.

**Do not re-test** bind address, `127.0.0.1` vs `localhost` at the server,
CORS preflight or the port. All four were measured and eliminated on
2026-09-17 (`46c176a`, `docs/demo-runbook.md:416-450`).

---

## Phase 1 — clean reset and launch (10 min, mostly waiting)

**VV-016 and VV-015 require a clean reseed, and VV-004 requires a reset**, so
the sitting starts from one. Doing it later would invalidate rows already
taken.

1. `docker compose up -d postgres redis nats` (Docker Desktop does not
   autostart on this box; **not** `make dev`, and not a bare
   `docker compose up` — the `backend` service fails to build).
2. Backend in its own window: `.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos`.
   **Record the new PID.** A restart is verified by identity, never by the port
   answering.
3. Clean reset: your usual `demo-reset.ps1 -Force` / `demo-up.ps1 -Release -Fresh`.
4. **Before letting the till drain**, take VV-016 part A below.
5. POS: launch the release binary yourself (`.\apps\pos\run-dev.ps1 -Release`,
   or the exe directly). Sign in `cashier@holler.test` / `holler123`.
6. Admin console and KDS as needed for phases 4–5.

**`HOLLER_DB_KEY_HEX` does not survive a new terminal**, and the bootstrap's
own refusal then advises minting a new key, which is **wrong on a machine that
already has a sealed database.** Read the existing one back — the
`Select-String` line is in `docs/RESUME.md`.

---

## Phase 2 — the till, before any order (12 min)

Five rows, all on the first screen after sign-in. No order needed.

| Row | Minutes | What to do | PASS | FAIL | Screenshot |
|---|---|---|---|---|---|
| **VV-010** | 1 | Look at the title bar and the taskbar icon | A real title and a real icon | The 16×16 scaffold placeholder. **Expected FAIL until artwork exists** (D23) — record it as a FAIL, it is not a surprise | `VV-010-window-chrome.png` |
| **VV-007** | 2 | Read the till header: restaurant name, address, GSTIN | All three match `seed/outlet.toml` | Any of them is a hardcoded or stale value | `VV-007-header.png` |
| **VV-017** | 3 | Select **Sushi Platter** in the left rail (a section with no Thai dish). Type `Thai` into **Search menu…** | Results appear **from other sections** — `Thai Grilled Chicken Salad`, `Pad Thai` — each card showing its **section name under the dish name**. Clear the box: the grid returns to Sushi Platter's two items with **no** section labels | An empty grid (the pre-`045b68e` behaviour, falsified by you on the live till on 2026-09-17), or results with no section label | `VV-017-search-cross-section.png` |
| **VV-018** | 2 | Scroll the left rail top to bottom | **No category named "… (internal -- not sold)"** — neither `Kitchen Prep` nor `Test fixtures`. Every other section still appears and still opens | Either internal section visible | `VV-018-category-rail.png` |
| **VV-019** | 2 | Read the rail's longest names: `Tartar & Carpaccio`, `Wok Poultry & Meat`, `Non Veg Tapas`, `Sushi Roll (4pc)` | Each reads **in full, no `…`**; grid and cart unchanged in width; nothing below the rail clipped | Any ellipsis, or a clipped rail | `VV-019-rail-long-names.png` |

**Note on VV-018:** the hiding is a **name match**, deliberately, so a category
renamed without the word "internal" comes back. A PASS here is a PASS for
tonight, not for the design — the flag on the row is filed post-demo.

---

## Phase 3 — order, kitchen, and the new listener (20 min)

**VV-020 is the row this run exists for, and its falsifier is the COLLAPSED
panel.** Take it before VV-012, because VV-012 asks you to open a panel and
`staleTime: 0` will then mask what VV-020 is testing.

| Row | Minutes | What to do | PASS | FAIL | Screenshot |
|---|---|---|---|---|---|
| **VV-020a** | 5 | Take a dine-in order with a modifier, send it to the kitchen. Return to the order list. **Leave every Kitchen panel COLLAPSED. Do not touch the window.** On the KDS, bump the ticket to READY. Wait ~5s. Then expand that order's Kitchen panel | The order row reflects the bump **without the panel ever having been open**, and expanding shows READY immediately | The row is stale until you expand, or shows an older status. **This is the pre-fix behaviour** | `VV-020a-collapsed-panel.png` |
| **VV-020b** | 4 | With the order list open and untouched, place an order from the **captain phone**. Do not send it to the kitchen. Wait up to 20s | The order appears in the POS order list on its own (the 15s fallback poll — a created-but-unsent order broadcasts no KOT, so no event fires) | It never appears until you navigate away and back | `VV-020b-captain-order.png` |
| **VV-012** | 3 | With a Kitchen panel **open** and untouched, acknowledge ticket #1 on the KDS. Do not switch panels, do not alt-tab | The status changes **on its own** within a second or two. **No remount** | Nothing moves until you collapse/expand. **Alt-tabbing is not a valid way to run this** — `refetchOnWindowFocus` is false, so a focus change refetches nothing and proves nothing | `VV-012-live-status.png` |
| **VV-014** | 2 | Look at a ticket in each status | Buttons read as **verbs** — Acknowledge, Start preparing, Mark ready, Mark served, Cancel ticket. Only legal moves offered. Badge **coloured AND worded** | A button named after a status, an illegal move offered, or colour with no word | `VV-014-kot-buttons.png` |
| **VV-013** | 3 | Force a stale view: with the panel open, acknowledge on the KDS then **immediately** press the till's own action for that ticket | If a rejection happens at all, the row is **already corrected** when the error appears and the offending button is gone | The error sits beside the stale status that caused it | `VV-013-refused-move.png` |
| **VV-005** | 3 | On the captain phone: send a round for table T1. **Without clearing the table**, send a second round | The second round **appends to the open order**; the KDS shows a **second ticket carrying only the new round** | A second order, or a ticket repeating round one | `VV-005-second-round.png` |

**VV-013 is hard to force once VV-012 works — that is the point.** It is the
backstop for when live update fails. If you cannot force it, record
`UNVERIFIABLE` with that reason rather than PASS.

---

## Phase 4 — the bill and the second device (10 min)

| Row | Minutes | What to do | PASS | FAIL | Screenshot |
|---|---|---|---|---|---|
| **VV-006** | 5 | Bill an order split cash + UPI. Open the PDF the print writes | The UPI QR is on **both** the invoice screen and the PDF, and **both name the same payee** | A QR on one and not the other, or two different payees | `VV-006-upi-qr.png` (both, side by side) |
| **VV-008** | 5 | Load the KDS in a browser on a **second device over the hotspot**. Send a ticket from the till. Bump it | The ticket renders, the bump sticks, and **no raw UUID, dev label or internal note** is on screen | Anything internal visible, or the bump not sticking | `VV-008-kds-second-device.png` |

**The KDS has never been observed on a second device.** Treat a failure here as
new information, not a regression.

---

## Phase 5 — sync, the banner, and the admin console (20 min)

**VV-016 part A must have been taken in phase 1, before the till drained.**

| Row | Minutes | What to do | PASS | FAIL | Screenshot |
|---|---|---|---|---|---|
| **VV-016a** | 3 | *(phase 1)* After the clean `-Fresh`, **before the till drains**, in the cloud: `SELECT count(*), min(entry_seq), max(entry_seq) FROM stock_ledger_entry WHERE outlet_id = '<demo outlet>';` | **0 rows.** An empty cloud ledger immediately after seeding is the INTENDED state | **45 rows before the till has drained means fix 5 has regressed** — stop and say so | `VV-016a-cloud-ledger-empty.png` |
| **VV-016b** | 4 | Let the till drain, re-run the query. Then record one wastage on the till, let it drain, re-run again | **45 rows, `entry_seq` 1–45, carrying the EDGE's row ids**; after the wastage, **46 rows, max `entry_seq` 46**. Banner attention list **EMPTY** throughout, `no_route` count only. **No `conflict (HTTP 409)` at any point** | Any 409, or ids that are not the edge's | `VV-016b-cloud-ledger-drained.png` |
| **VV-011** | 2 | Watch the sync banner through the whole sitting | A muted line reads **"N records kept locally — the cloud has no route for them yet. Nothing to do."** and the **attention list stays EMPTY** | A red count, or a growing list of rows nobody can act on | `VV-011-sync-banner.png` |
| **VV-001** | 5 | Stop the cloud. Take an order. Restart the cloud (`dev-up.ps1`, **confirm a NEW pid**). Wait one pump interval | The banner **empties**. No `conflict (HTTP 409)` at any point | The banner keeps rows, or a 409 appears | `VV-001-banner-empties.png` |
| **VV-003** | 1 | Read an order number on the order list and in the banner | **`#A2`**, never `##A2`, on both surfaces | A doubled `#` on either | `VV-003-order-number.png` |
| **VV-002** | 2 | After VV-001, open the admin Orders tab and find that order | Listed, and its status is **not DRAFT** — it reads what the till showed | DRAFT, or absent | `VV-002-admin-order-status.png` |
| **VV-004** | 2 | In admin Orders, after the reset and one end-to-end order | **No order with a non-zero total and zero lines.** 21 had that before the reset | Any such order | `VV-004-admin-no-empty-orders.png` |
| **VV-015** | 1 | In admin Orders, read the device column | The order **names the device** — "Dev Till 1" | A blank or a raw UUID | `VV-015-admin-device-name.png` |

**VV-015 needs the reseed**, which phase 1 did. The row is seeded UNENROLLED,
which is correct and **must not** read as a device that has enrolled.

---

## Phase 6 — the exit probes. LAST, because they end the session (8 min)

These two settle A6, which is **BLOCKED on exactly this observation**. Do them
only when every row above is taken.

**Before you start, note the two paths:**
`%APPDATA%\com.holler.pos\edge.db` (plaintext — must NOT exist after a clean
exit) and `%APPDATA%\com.holler.pos\edge.db.enc` (sealed).

### A6-1 — does closing the window seal the database? (4 min)

1. With the POS running, check: does `edge.db` exist right now? (It should,
   while the app is open — that is normal.)
2. Note the POS **PID**.
3. **Close the window with the X button.**
4. Wait 10 seconds. Then: `Get-Process -Id <pid>` — **is the process still
   alive?**
5. List the directory: is `edge.db` gone, and is `edge.db.enc` newer than it
   was?

**PASS:** the process is gone **and** `edge.db` (with its `-wal`/`-shm`) is
gone, `edge.db.enc` freshly written.
**FAIL:** the process is still alive after the window closed (**this is what
was recorded on 2026-09-17**), or the plaintext remains.

**Record the PID and the `Get-Process` output** — that single line is the whole
diagnosis. A process that outlives its window means the Tauri event loop never
ended, which is why `RunEvent::Exit` never fires and why the (correct,
complete) exit hook at `lib.rs:199-243` never runs.

**Screenshot:** `A6-1-window-close.png` — the directory listing and the
`Get-Process` result in one frame.

### A6-2 — does Ctrl+C seal the database? (4 min)

1. Relaunch the POS **from a terminal** so it has a console attached.
2. Confirm `edge.db` exists.
3. Press **Ctrl+C** in that terminal.
4. List the directory again.

**PASS:** `edge.db` is gone and `edge.db.enc` is fresh.
**FAIL (expected):** `edge.db` remains. **No signal handler is installed
anywhere in the tree**, so SIGINT ends the process before any Rust destructor
or Tauri hook runs. This row is expected to fail and the value is confirming
it, so the fix can be written against a known cause rather than a guess.

**Screenshot:** `A6-2-ctrl-c.png`.

### What a FAIL here does and does not mean

**No data is lost either way.** `recover_crash_leftovers`
(`edge/database/src/crypto.rs:312`) proves the key, folds any WAL pages in and
**reseals the leftover** on the next open; an unreadable leftover is
quarantined with its bytes intact, never deleted. The real harm is narrower
than the carried summary says:

1. A decrypted SQLite file holding cached **Argon2id credential hashes** sits
   on disk in the clear between an abnormal exit and the next open (ADR-011).
2. **A backup that copies only `edge.db.enc` silently loses the last
   session**, because the newer state is in the plaintext beside it. Take both
   files, or take the backup after a verified clean exit.

---

## After the sitting

Paste the results back as a list — row id, PASS/FAIL/UNVERIFIABLE, and the
screenshot filename. Each row will then be marked in `docs/visual-verify.md`
with your screenshot as **Executed** evidence and you as the named verifier,
every FAIL filed as a bug row in `docs/backlog.md`, and `docs/RESUME.md`
updated.

**Rows expected to FAIL, so a failure is information rather than a surprise:**
VV-010 (no artwork yet, D23) and A6-2 (no signal handler exists).
