"""Assemble docs/demo-scenarios.xlsx from the scenario run's result log.

Reads .scenario-results/results.jsonl (one row per recorded scenario, append
only) and writes a two-sheet workbook: Scenarios and Summary.

RE-RUNS OVERWRITE, THEY DO NOT ACCUMULATE. The log is append-only so a stage
can be re-run after a fix, and the LAST row recorded for an id wins. A sheet
that carried both the stale row and the fresh one would let a reader quote
whichever supported their conclusion.

If openpyxl is unavailable this writes docs/demo-scenarios.csv instead and
says so on stdout — never silently.
"""

from __future__ import annotations

import json
import os
import sys
from collections import OrderedDict

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RESULTS = os.path.join(REPO, ".scenario-results", "results.jsonl")
XLSX = os.path.join(REPO, "docs", "demo-scenarios.xlsx")
CSV = os.path.join(REPO, "docs", "demo-scenarios.csv")

COLUMNS = [
    ("id", "ID"),
    ("demoStep", "Demo step"),
    ("scenario", "Scenario"),
    ("surface", "Surface"),
    ("precondition", "Precondition"),
    ("steps", "Steps"),
    ("expected", "Expected"),
    ("actual", "Actual"),
    ("status", "Status"),
    ("evidence", "Evidence"),
    ("notes", "Notes"),
    ("rerun", "Re-run"),
]

# Group order for the sheet: environment first, then the paths in demo order.
GROUP_ORDER = ["S-ENV", "S-BE", "S-CAP", "S-CUI", "S-KDS", "S-ADM", "S-API", "S-SYNC", "S-CHAIN"]

# Placeholder ids a later run replaced with real rows, and which the
# stage-supersede rule cannot drop on its own.
#
# The rule keys on (stage, runId). Rows recorded before those stamps existed
# all carry stage "unknown", so a stale placeholder from the FIRST pass sits in
# the same group as every other unstamped row and survives every re-run. That
# is how S-CAP-BLOCKED — "pairing failed at S-CAP-03, so no authenticated
# captain call can be made" — was still in the sheet beside S-CAP-03 PASS.
# Retiring it by name, with the reason, is the honest fix: the ids that
# replaced it (S-CAP-11..15) carry the real verdicts.
RETIRED_IDS = {
    "S-CAP-BLOCKED": (
        "First-pass placeholder for 'everything after pairing'. The re-run paired "
        "successfully and recorded S-CAP-04..15 individually, so this row is "
        "superseded by rows that say more than it did."
    ),
}

# Why a row's status moved between the first pass and the re-run. Merged into
# the Re-run column at build time for rows whose stage script does not write
# one itself, so the explanation lives in the repository rather than in a chat
# transcript (CLAUDE.md: the chat is not the record).
RERUN_NOTES = {
    "S-ENV-01": (
        "WAS FAIL (captain, POS Vite and the KDS socket all down; holler-pos not running). "
        "NOW PASS — re-run against holler-pos pid 80612, started 20:01:36 IST, with 9310 and "
        "9320 both listening. The pid was verified by identity, not by the port answering."
    ),
    "S-ENV-02": (
        "WAS FAIL (the POS process vanished mid-run, between 19:52 and 19:58 IST). NOW PASS — "
        "pid 80612 held the whole re-run: it was 80612 at the first probe and 80612 at the last."
    ),
    "S-BE-04": (
        "WAS FAIL (HTTP 401). NOW PASS with the same credentials and no change to the backend — "
        "the 401 was the login rate-limit window, not a credential fault, which is exactly the "
        "ambiguity S-BE-09 records as a finding. Waiting out the window was the only fix applied."
    ),
    "S-CUI-03": (
        "WAS FAIL — 'That device token was rejected', the listener answering 401 'device "
        "credential not cached locally'. NOW PASS: the same token reaches the Tables screen. "
        "The credential is present at the edge now and was not before. THE AUTH PATH WAS NEVER "
        "BROKEN — the credential had simply not been pulled yet."
    ),
    "S-CUI-08": (
        "WAS BLOCKED by S-CUI-03 (no authenticated screen rendered). NOW FAIL on its own "
        "evidence, one step further on: the page reaches the cart and the send is refused with "
        "'sqlite error: FOREIGN KEY constraint failed'. Root cause isolated in S-CAP-19."
    ),
    "S-CUI-04": "WAS BLOCKED by S-CUI-03. NOW PASS — pairing succeeded, so the screen renders.",
    "S-CUI-05": "WAS BLOCKED by S-CUI-03. NOW PASS — pairing succeeded, so the screen renders.",
    "S-CUI-07": "WAS BLOCKED by S-CUI-03. NOW PASS — pairing succeeded, so the screen renders.",
    "S-CUI-06": (
        "WAS BLOCKED by S-CUI-03. NOW NOT TESTABLE, which is a different thing and a weaker "
        "result than a pass: the screen renders, but no item in this catalogue is snoozed, so "
        "the page's is_available handling was never exercised. Recorded as unexercised rather "
        "than assumed working."
    ),
    "S-KDS-04": (
        "WAS FAIL (data-status='disconnected'). NOW PASS — the socket is up because the POS "
        "process hosting it is up. The earlier failure was the dead process, not the KDS."
    ),
    "S-KDS-05": (
        "WAS FAIL (read as a KDS defect). NOW BLOCKED, and the demotion is the point: the edge's "
        "own snapshot carries 0 tickets, so there is nothing for the board to render and the KDS "
        "is behaving correctly. The board cannot be judged until S-CHAIN-02 can put a ticket on it."
    ),
    "S-KDS-06": "STILL BLOCKED, new reason: not the dead POS but S-CHAIN-02 — no order can be created, so no ticket exists to bump.",
    "S-KDS-07": "STILL BLOCKED, new reason: not the dead POS but S-CHAIN-02 — no ticket exists to bump, so no bump can be reloaded.",
    "S-KDS-02": "STILL BLOCKED, new reason: pairing now works, so the block moved from S-CUI-03 to the order-create foreign key in S-CAP-19.",
    "S-ADM-02": (
        "WAS FAIL ('Sign-in failed…'). NOW PASS with unchanged credentials — the earlier failure "
        "was the shared login rate-limit budget, indistinguishable by design from a wrong "
        "password (S-BE-09). Nothing was reconfigured; the window rolled."
    ),
    "S-ADM-03": "WAS BLOCKED by S-ADM-02. NOW PASS — sign-in succeeded, so the screen loads real rows.",
    "S-ADM-04": "WAS BLOCKED by S-ADM-02. NOW PASS — a price edit was written and read back from the backend.",
    "S-ADM-05": "WAS BLOCKED by S-ADM-02. NOW PASS — sign-in succeeded, so the screen loads.",
    "S-ADM-06": "WAS BLOCKED by S-ADM-02. NOW PASS — sign-in succeeded, so the screen loads.",
    "S-ADM-07": "WAS BLOCKED by S-ADM-02. NOW PASS — sign-in succeeded, so the screen could be inspected.",
    "S-SYNC-09": (
        "WAS FAIL — 2 contacts in 6 minutes, longest gap 12m27s against a documented 60s, with "
        "the shutdown-drain alternative explicitly NOT excluded. NOW PASS: 7 contacts in 7 "
        "minutes, every gap 1m0s, measured on Postgres's clock. "
        "VERDICT ON STARTUP-PULL VERSUS PERIODIC-PULL: THE A5 PERIODIC PUMP IS NOT DEFECTIVE. "
        "It ticks at its documented interval on a healthy process. The earlier 12m27s gap "
        "belonged to the PREVIOUS process, which stopped pumping and then died minutes later — "
        "an instance that was already failing, not a design fault. So the pairing delay was a "
        "property of that sick process, not of the pump. NOTE WHAT THIS RUN DOES NOT SHOW: "
        "stage 01 REUSED the existing enrolment rather than minting a new credential, so no "
        "credential arrived from 401 to 200 during this run and the periodic pull was never "
        "watched delivering one. That it does is inferred from the proven 60s tick plus "
        "edge/sync/src/config.rs:770, not observed."
    ),
    "S-SYNC-02": (
        "STILL BLOCKED, new reason: not 'S-CAP-10 was blocked' by a dead process but the "
        "order-create foreign key of S-CAP-19. No captain order exists to look for in Postgres."
    ),
    "S-CAP-10": (
        "STILL FAIL, but no longer for an unknown reason. The 400 is a sqlite FOREIGN KEY "
        "violation and the column is isolated in S-CAP-19."
    ),
    **{
        rid: (
            "STILL BLOCKED, new reason. It was blocked by pairing (the phone could not "
            "authenticate at all); the phone now authenticates, lists tables and lists the menu, "
            "and this is blocked one step further on by the order-create foreign key in S-CAP-19."
        )
        for rid in ("S-CAP-11", "S-CAP-12", "S-CAP-13", "S-CAP-14", "S-CAP-15")
    },
}

# Failures ranked by what they cost the demo, most severe first. An id absent
# from this list sorts after every id present in it.
SEVERITY = [
    "S-CHAIN-02", # the demo's central claim does not complete
    "S-CAP-19",   # ...and this is the column that stops it: order.device_id
    "S-CAP-10",   # the waiter phone cannot create an order at all
    "S-CAP-18",   # the same failure, seen across five request shapes
    "S-CUI-08",   # the same failure, seen on the real phone screen
    "S-SYNC-10",  # orders reach the cloud with a total and no lines
    "S-SYNC-11",  # the cloud holds no tables and no stations at all
    "S-SYNC-12",  # a live POS stopped pumping while every health signal stayed green
    "S-ENV-02",   # the POS process vanished mid-run
    "S-SYNC-04",  # gap A7: nothing but `order` reaches the cloud
    "S-ADM-08",   # demo step 4 has no screen
    "S-ADM-09",   # demo step 5 has no screen
    "S-SYNC-03",  # replayed orders carry an unresolvable device_id
    "S-SYNC-08",  # 12 cloud items have no variant, so their lines cannot replay
    "S-KDS-04",
    "S-KDS-05",
    "S-ADM-02",
    "S-BE-09",
    "S-ENV-01",
]


def load_rows():
    if not os.path.exists(RESULTS):
        sys.exit(f"no results at {RESULTS} — run the scenario stages first")
    raw = []
    # utf-8-sig: the log is appended to by Node, but a PowerShell truncation
    # of the file can leave a byte-order mark on the first line.
    with open(RESULTS, "r", encoding="utf-8-sig") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            raw.append(json.loads(line))

    # A STAGE'S RE-RUN SUPERSEDES EVERY ROW IT PREVIOUSLY WROTE, not merely the
    # ids it happens to write again. Otherwise a stage that failed early and
    # emitted a placeholder leaves that row behind forever, because the
    # successful re-run never emits that id to overwrite it — and the sheet
    # then shows one path both working and blocked at once.
    newest_run = {}
    for row in raw:
        stage = row.get("stage", "unknown")
        run = row.get("runId", "")
        if run > newest_run.get(stage, ""):
            newest_run[stage] = run

    latest = OrderedDict()
    dropped = 0
    for row in raw:
        stage = row.get("stage", "unknown")
        if row.get("runId", "") != newest_run.get(stage, ""):
            dropped += 1
            continue
        latest[row["id"]] = row  # last write wins within the surviving run
    if dropped:
        print(f"(superseded by a later run of the same stage: {dropped} row(s))")

    for rid, reason in RETIRED_IDS.items():
        if latest.pop(rid, None) is not None:
            print(f"(retired placeholder {rid}: {reason})")

    # A curated Re-run note never overwrites one the stage script wrote itself —
    # the script was there when the row was produced and this table was not.
    for rid, note in RERUN_NOTES.items():
        row = latest.get(rid)
        if row is not None and not row.get("rerun"):
            row["rerun"] = note

    return list(latest.values())


def sort_key(row):
    rid = row["id"]
    prefix = rid.rsplit("-", 1)[0]
    try:
        group = GROUP_ORDER.index(prefix)
    except ValueError:
        group = len(GROUP_ORDER)
    tail = rid.rsplit("-", 1)[-1]
    return (group, int(tail) if tail.isdigit() else 999, rid)


def severity_key(row):
    try:
        return SEVERITY.index(row["id"])
    except ValueError:
        return len(SEVERITY)


def write_csv(rows):
    import csv

    with open(CSV, "w", encoding="utf-8", newline="") as fh:
        w = csv.writer(fh)
        w.writerow([h for _, h in COLUMNS])
        for r in rows:
            w.writerow([r.get(k, "") for k, _ in COLUMNS])
    print(f"WROTE CSV (openpyxl unavailable): {CSV}")


def write_xlsx(rows, counts, failures):
    from openpyxl import Workbook
    from openpyxl.styles import Alignment, Font, PatternFill
    from openpyxl.utils import get_column_letter

    wb = Workbook()
    ws = wb.active
    ws.title = "Scenarios"

    header_fill = PatternFill("solid", fgColor="1F2937")
    header_font = Font(color="FFFFFF", bold=True, size=11)
    status_fill = {
        "PASS": PatternFill("solid", fgColor="D1FAE5"),
        "FAIL": PatternFill("solid", fgColor="FEE2E2"),
        "BLOCKED": PatternFill("solid", fgColor="FEF3C7"),
        "NOT TESTABLE": PatternFill("solid", fgColor="E5E7EB"),
    }
    status_font = {
        "PASS": Font(color="065F46", bold=True),
        "FAIL": Font(color="991B1B", bold=True),
        "BLOCKED": Font(color="92400E", bold=True),
        "NOT TESTABLE": Font(color="374151", bold=True),
    }

    ws.append([h for _, h in COLUMNS])
    for cell in ws[1]:
        cell.fill = header_fill
        cell.font = header_font
        cell.alignment = Alignment(vertical="center")
    ws.freeze_panes = "A2"

    for r in rows:
        ws.append([r.get(k, "") for k, _ in COLUMNS])
        row_idx = ws.max_row
        st = r.get("status", "")
        c = ws.cell(row=row_idx, column=9)
        if st in status_fill:
            c.fill = status_fill[st]
            c.font = status_font[st]
        c.alignment = Alignment(horizontal="center", vertical="top")
        for col in range(1, len(COLUMNS) + 1):
            ws.cell(row=row_idx, column=col).alignment = Alignment(
                vertical="top", wrap_text=col in (3, 5, 6, 7, 8, 10, 11, 12)
            )

    widths = [11, 11, 44, 26, 40, 42, 40, 60, 14, 40, 62, 64]
    for i, w in enumerate(widths, start=1):
        ws.column_dimensions[get_column_letter(i)].width = w
    ws.auto_filter.ref = f"A1:L{ws.max_row}"

    # ------------------------------------------------------------- Summary
    s = wb.create_sheet("Summary")
    title = Font(bold=True, size=13)
    s["A1"] = "Holler demo scenario run"
    s["A1"].font = Font(bold=True, size=15)
    s["A2"] = "Every row in Scenarios was executed against the live stack. Nothing is mocked."
    s["A3"] = "A scenario that could not be driven is NOT TESTABLE or BLOCKED, never PASS."
    s["A4"] = (
        "RE-RUN 2026-09-11 against holler-pos pid 80612 (started 20:01:36 IST). The Re-run column "
        "says why each moved row moved; an empty Re-run cell means the row was recorded once and "
        "never revisited, not that it was re-confirmed."
    )

    s["A5"] = "Counts by status"
    s["A5"].font = title
    s.append([])
    r = 6
    for st in ["PASS", "FAIL", "BLOCKED", "NOT TESTABLE"]:
        s.cell(row=r, column=1, value=st).font = Font(bold=True)
        s.cell(row=r, column=2, value=counts.get(st, 0))
        if st in status_fill:
            s.cell(row=r, column=1).fill = status_fill[st]
        r += 1
    s.cell(row=r, column=1, value="TOTAL").font = Font(bold=True)
    s.cell(row=r, column=2, value=sum(counts.values())).font = Font(bold=True)

    r += 2
    s.cell(row=r, column=1, value="Top failures, most severe first").font = title
    r += 1
    hdr = ["ID", "Demo step", "Scenario", "What actually happened"]
    for i, h in enumerate(hdr, start=1):
        c = s.cell(row=r, column=i, value=h)
        c.fill = header_fill
        c.font = header_font
    r += 1
    for f in failures:
        s.cell(row=r, column=1, value=f["id"])
        s.cell(row=r, column=2, value=f.get("demoStep", ""))
        s.cell(row=r, column=3, value=f["scenario"]).alignment = Alignment(wrap_text=True, vertical="top")
        s.cell(row=r, column=4, value=f["actual"]).alignment = Alignment(wrap_text=True, vertical="top")
        s.cell(row=r, column=1).fill = status_fill["FAIL"]
        r += 1

    for col, w in zip("ABCD", [14, 12, 46, 96]):
        s.column_dimensions[col].width = w

    wb.save(XLSX)
    print(f"WROTE XLSX: {XLSX}")


def main():
    rows = sorted(load_rows(), key=sort_key)
    counts = {}
    for r in rows:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    failures = sorted([r for r in rows if r["status"] == "FAIL"], key=severity_key)

    for st in ["PASS", "FAIL", "BLOCKED", "NOT TESTABLE"]:
        print(f"{st:<14} {counts.get(st, 0)}")
    print(f"{'TOTAL':<14} {len(rows)}")

    try:
        write_xlsx(rows, counts, failures)
    except ImportError:
        write_csv(rows)


if __name__ == "__main__":
    main()
