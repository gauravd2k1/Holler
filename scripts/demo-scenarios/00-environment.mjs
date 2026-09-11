/**
 * Stage 0 — what was actually up when this run happened.
 *
 * Recorded as scenario rows because the alternative is a sheet whose FAILs
 * cannot be told apart from an absent service. A criterion that a stopped
 * process also fails is not evidence about the product, and the only way a
 * later reader can separate the two is a probe taken at the same time.
 *
 * Every port is probed by CONNECTING, not by reading a process list: the task
 * handed over a POS pid that no longer exists, and a stale pid reads exactly
 * like a healthy one until something tries to talk to it.
 */
import { execFileSync } from "node:child_process";
import { ADMIN_UI, BACKEND, CAPTAIN, KDS_UI, record, sleep } from "./lib.mjs";

async function probeHttp(url) {
  try {
    const r = await fetch(url, { signal: AbortSignal.timeout(4000) });
    return { up: true, detail: `HTTP ${r.status}` };
  } catch (e) {
    return { up: false, detail: String(e.cause?.code ?? e.message).slice(0, 60) };
  }
}

async function probeWs(url) {
  return new Promise((resolve) => {
    let ws;
    const done = (up, detail) => { try { ws?.close(); } catch { /* closing a dead socket */ } resolve({ up, detail }); };
    const t = setTimeout(() => done(false, "no response within 4s"), 4000);
    try {
      ws = new WebSocket(url);
      ws.onopen = () => { clearTimeout(t); done(true, "socket opened"); };
      ws.onerror = () => { clearTimeout(t); done(false, "connection refused or handshake rejected"); };
    } catch (e) {
      clearTimeout(t);
      done(false, String(e.message).slice(0, 60));
    }
  });
}

function posProcess() {
  try {
    const out = execFileSync("powershell", ["-NoProfile", "-Command",
      "Get-Process -Name holler-pos -ErrorAction SilentlyContinue | ForEach-Object { \"$($_.Id)@$($_.StartTime.ToString('o'))\" }"],
      { encoding: "utf8" }).trim();
    return out === "" ? null : out;
  } catch {
    return null;
  }
}

const run = async () => {
  const probes = {
    backend: await probeHttp(`${BACKEND}/health`),
    captain: await probeHttp(`${CAPTAIN}/`),
    kdsUi: await probeHttp(KDS_UI),
    adminUi: await probeHttp(ADMIN_UI),
    posVite: await probeHttp("http://localhost:5173/"),
    kdsSocket: await probeWs("ws://localhost:9310/kds?outlet_id=probe&device_id=probe"),
  };
  const pos = posProcess();
  const summary = Object.entries(probes).map(([k, v]) => `${k}=${v.up ? "UP" : `DOWN(${v.detail})`}`).join(", ");

  record({
    id: "S-ENV-01",
    demoStep: "pre",
    scenario: "Record which services were actually reachable when this run happened",
    surface: "all",
    precondition: "The task handed over: backend 8080 pid 69896, POS holler-pos pid 78900 hosting 9310 and 9320, Vite on 5173/5174/5175",
    steps: "Connect to each port; look up the holler-pos process by name, not by the handed-over pid",
    expected: "All six surfaces reachable, as handed over",
    actual: `${summary}. holler-pos process: ${pos ?? "NOT RUNNING"}`,
    status: Object.values(probes).every((p) => p.up) ? "PASS" : "FAIL",
    evidence: summary,
    notes:
      "THE HANDED-OVER PID WAS ALREADY WRONG: the task named holler-pos pid 78900; the process actually running at " +
      "the start of this run was pid 74104, started 19:34:44 IST. A restart is verified by the new process's " +
      "identity, never by the port answering (CLAUDE.md), and this is the same trap one step earlier — a pid quoted " +
      "from an earlier session reads exactly like a live one.",
  });

  record({
    id: "S-ENV-02",
    demoStep: "pre",
    scenario: "The POS process stayed up for the whole run",
    surface: "POS process (9310 + 9320 + 5173)",
    precondition: "holler-pos pid 74104 was serving 9320 at 19:52 IST — S-CUI-01 recorded HTTP 200 and a real captain 401 body",
    steps: "Re-probe 9320, 9310 and 5173 after the captain stage",
    expected: "Still up, so later FAILs are attributable to the product",
    actual: pos
      ? `holler-pos running: ${pos}`
      : "holler-pos is NOT RUNNING. 9320, 9310 and 5173 all refuse connections. The process was alive at 19:52 IST " +
        "(S-CUI-01/02/03 drove it and recorded real listener responses) and was gone by 19:58 IST. This suite did " +
        "not stop it — it starts, kills and reconfigures nothing by instruction.",
    status: pos ? "PASS" : "FAIL",
    evidence: `9320 ${probes.captain.detail}; 9310 ${probes.kdsSocket.detail}; 5173 ${probes.posVite.detail}`,
    notes:
      "CONSEQUENCE FOR THIS SHEET: every captain and KDS row recorded after this point is BLOCKED by an absent " +
      "process, not by a product defect. The rows captured before it — S-CAP-01..03 and S-CUI-01..03 — were taken " +
      "against the live listener and stand on their own.",
  });

  await sleep(100);
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
