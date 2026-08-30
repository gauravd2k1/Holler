//! M4 acceptance criterion 6: "Ledger entries created at the edge replay to
//! the cloud and read back identically."
//!
//! WHAT MAKES THIS DIFFERENT FROM `ranged_replay.rs`. That file points the
//! pump at a `tiny_http` responder that answers 201 to anything. It proves the
//! pump's control flow -- cursors, per-entry budgets, blocking -- and cannot
//! prove one byte of the conversation, because the fake agrees with whatever
//! the edge says. Every Rust-side cloud interaction in this repo was against
//! that fake, so the fake was the risk.
//!
//! Here the pump talks to the REAL `cmd/api` binary, built from this checkout,
//! over a real socket, against a real PostgreSQL, holding a real device
//! credential minted by the real enrollment route. Nothing is stubbed. The
//! entry is not placed either: it is earned through `Db::record_wastage`, the
//! shipping edge API, so its `entry_seq` comes from the real counter.
//!
//! READ-BACK IS TWO CHECKS, BECAUSE THERE ARE TWO WAYS TO LOSE A FIELD.
//!
//!   1. The 201 echo. `IngestLedgerEntry` returns the entry it was handed, so
//!      the echo is the payload round-tripped through Go's `StockLedgerEntry`
//!      -- it proves WIRE fidelity, and a field the Go type does not declare
//!      vanishes here. It proves nothing about storage.
//!   2. The row, read straight out of PostgreSQL. That proves STORAGE
//!      fidelity: what the INSERT actually persisted.
//!
//! THE FIXTURE MUST POPULATE THE FIELDS, TOO. Both comparisons are
//! whole-object, and a whole-object comparison of a row whose optional fields
//! are all null cannot tell a field that was carried from a field neither side
//! has. That is how `source_stock_count_id` was dropped by the cloud for two
//! milestones while this test was green (contracts 0.5.9), and then how the
//! three 0.6.0 procurement columns were dropped after it -- the fixture was a
//! wastage row, on which all three are legitimately null, so the test passed
//! on null agreeing with null and proved nothing.
//!
//! So the fixture is FIVE entries, one per provenance group: a wastage row, a
//! count-sourced COUNT_ADJUSTMENT, a PURCHASE from a real receipt, a
//! RETURN_TO_VENDOR from a real return, and a TRANSFER_OUT from a real
//! dispatch. **A provenance group added later needs its own entry here, and
//! the group is not landed until it has one.** A fidelity test proves fidelity
//! only for the fields its fixture populates.
//!
//! Between them nothing in the round trip is unchecked. Both comparisons are
//! whole-object and byte-exact against a canonical serialisation -- never a
//! field-by-field spot check, because "identically" then quietly degrades to
//! "the fields I remembered to assert", and the fields nobody remembers are
//! exactly the ones that get dropped.
//!
//! GATED, AND CI INVOKES THE GATE. This test needs PostgreSQL, the Go
//! toolchain and a built `cmd/api` -- none of which the `edge` job has -- so
//! it sits behind the `cloud-e2e` feature. A gated target nothing invokes is
//! the state criterion 1 spent four pushes in, so `scripts/check-gated-tests.mjs`
//! fails the build if this target is not named on a ci.yml run line.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use holler_edge_database::{model, repo, Db};
use holler_edge_sync::worker::{SyncWorker, WorkerConfig};
use postgres::{Client as PgClient, NoTls};
use serde_json::{json, Value};

const OUTLET_TZ: &str = "Asia/Kolkata";
const BOOT_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- ids ------

/// A UUIDv7-shaped id (§74: app-generated, time-sortable, never a DB default).
///
/// Minted rather than fixed for the reason `internal/inventory/postgres_test.go`
/// records in its own `newULID`: this suite runs against a PERSISTENT database,
/// so any deterministic id collides with a previous run's leftovers the moment
/// the process restarts. A test that only passes on a clean database passes
/// forever in CI and fails for every human -- see docs/retro.md 2026-08-23.
fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = chrono::Utc::now().timestamp_millis() as u64 & 0xFFFF_FFFF_FFFF;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = u64::from(std::process::id());
    let a = (ms >> 16) as u32;
    let b = (ms & 0xFFFF) as u16;
    let c = 0x7000u16 | ((n as u16) & 0x0FFF);
    let d = 0x8000u16 | ((pid as u16) & 0x0FFF);
    let e = ((pid << 24) ^ (n << 12) ^ ms) & 0xFFFF_FFFF_FFFF;
    format!("{a:08x}-{b:04x}-{c:04x}-{d:04x}-{e:012x}")
}

// ------------------------------------------------- canonical comparison -----

/// Serialises a JSON value with every object key sorted, at every depth, so
/// two values can be compared as bytes rather than as trees.
///
/// Explicitly sorted rather than trusting `serde_json`'s default map: enabling
/// its `preserve_order` feature anywhere in the dependency graph would flip
/// that default to insertion order and turn this comparison into a comparison
/// of field ORDER, which would then fail for a reason that has nothing to do
/// with the round trip.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            let inner: Vec<String> = sorted
                .iter()
                .map(|(k, v)| format!("{}:{}", Value::String((*k).clone()), canonical(v)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", inner.join(","))
        }
        other => other.to_string(),
    }
}

fn assert_identical(label: &str, expected: &Value, actual: &Value) {
    let (e, a) = (canonical(expected), canonical(actual));
    if e != a {
        panic!(
            "{label}: the round trip is not identity.\n  sent:      {e}\n  came back: {a}\n\
             \n  Whole-object comparison is the point -- if this fails on a field you did not \
             expect to matter, that field is being lost somewhere on the seam."
        );
    }
}

// ------------------------------------------------------------ fixtures -----

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolving repo root")
}

/// FAILS on an unset variable rather than skipping, matching
/// `backend/internal/platform/testdb::RequireDatabaseURL`. A skip here would
/// reproduce the M2 "a skip is not a pass" failure this repo already
/// institutionalised out of the backend job.
fn require_database_url() -> String {
    match std::env::var("HOLLER_TEST_DATABASE_URL") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => panic!(
            "HOLLER_TEST_DATABASE_URL is required for the cloud-e2e tests and must not be \
             skipped past.\n  docker compose up -d postgres\n  \
             $env:HOLLER_TEST_DATABASE_URL=\"postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable\""
        ),
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserving a port");
    listener.local_addr().expect("reading reserved port").port()
}

fn run(label: &str, cmd: &mut Command) -> String {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("{label}: could not start ({e}). Is the Go toolchain on PATH?"));
    if !out.status.success() {
        panic!(
            "{label} failed ({})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What `cmd/devseed` prints between its `---HOLLER-DEVSEED---` markers. The
/// seed is the bootstrap: a tenant with no user cannot log in, and without a
/// login nothing can enroll a device, so the cloud cannot be reached at all.
struct Seed {
    tenant_id: String,
    outlet_id: String,
    email: String,
    password: String,
}

fn devseed(root: &Path, db_url: &str) -> Seed {
    let stdout = run(
        "go run ./cmd/devseed",
        Command::new("go")
            .args(["run", "./cmd/devseed", "--database-url", db_url])
            .current_dir(root.join("backend")),
    );

    let mut fields = BTreeMap::new();
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once('=') {
            fields.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let take = |k: &str| {
        fields
            .get(k)
            .unwrap_or_else(|| panic!("devseed printed no {k}:\n{stdout}"))
            .clone()
    };
    Seed {
        tenant_id: take("HOLLER_TENANT_ID"),
        outlet_id: take("HOLLER_OUTLET_ID"),
        email: take("HOLLER_SEED_EMAIL"),
        password: take("HOLLER_SEED_PASSWORD"),
    }
}

/// The seeded cashier holds `order.create`/`order.modify`/`table.manage` and
/// nothing else, so out of the box it can neither enroll a device
/// (`outlet.manage`) nor create an inventory item (`inventory.manage`).
///
/// Granted here in SQL rather than by widening `cmd/devseed`, which is a
/// shipping dev tool whose permission set is a product decision, not this
/// test's to make. Worth noting as a real gap though: with devseed alone a
/// developer cannot enroll a device against their own dev outlet.
fn grant_permissions(pg: &mut PgClient, tenant_id: &str) {
    for permission in ["outlet.manage", "inventory.manage"] {
        pg.execute(
            "INSERT INTO role_permission (role_id, permission)
             SELECT id, $2 FROM role WHERE tenant_id::text = $1
             ON CONFLICT DO NOTHING",
            &[&uuid_param(tenant_id), &permission],
        )
        .expect("granting permission");
    }
}

/// `postgres` has no UUID type without the `with-uuid` feature, so ids cross
/// the wire as TEXT and the COLUMN is cast, never the parameter: `$1::uuid`
/// makes the driver infer a uuid parameter and reject the string outright,
/// whereas `id::text = $1` compares in text on both sides. Kept in one place
/// so it stays consistent with the `::text` projections on the way back out.
fn uuid_param(s: &str) -> String {
    s.to_string()
}

// ------------------------------------------------------- isolated database --

/// A PostgreSQL database created for one test and dropped with it.
///
/// NOT a nicety. `entry_seq` is a PER-OUTLET counter and the cloud remembers
/// the high-water mark, while each run gets a fresh in-memory edge database
/// whose counter restarts at 1. Point two runs at one database and the second
/// one's entry 1 is a mark the cloud already holds under another id -- a
/// correct 409, for a reason that is entirely the test's fault.
///
/// The alternative is a test that only passes on a clean database, which
/// passes forever in CI (new container every run) and fails for every human
/// on the second `go test`. That is a defect this repo has already paid for
/// once (docs/retro.md 2026-08-23), so this makes its own clean database
/// rather than depending on one.
struct TempDatabase {
    admin: PgClient,
    name: String,
    url: String,
}

impl TempDatabase {
    fn create(admin_url: &str) -> Self {
        let mut admin = PgClient::connect(admin_url, NoTls).expect("connecting to postgres");
        let name = format!("holler_c6_{}", new_id().replace('-', ""));
        admin
            .batch_execute(&format!("CREATE DATABASE \"{name}\""))
            .unwrap_or_else(|e| panic!("creating {name}: {e}"));
        let url = replace_database(admin_url, &name);
        Self { admin, name, url }
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        // FORCE, because a lingering connection would otherwise leave the
        // database behind and the next run would find the instance full of
        // them. Best effort: a failure here must not mask a test failure.
        let _ = self.admin.batch_execute(&format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.name
        ));
    }
}

/// Swaps the database name in a libpq URL, leaving user, host, port and query
/// parameters (notably `sslmode`) exactly as given.
fn replace_database(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let root = base
        .rsplit_once('/')
        .map(|(head, _)| head)
        .unwrap_or(base)
        .to_string();
    match query {
        Some(q) => format!("{root}/{name}?{q}"),
        None => format!("{root}/{name}"),
    }
}

// ------------------------------------------------------------- the cloud ---

/// The real `cmd/api`, built and spawned. Killed on drop -- a leaked backend
/// holds the port and the next run fails for a reason that has nothing to do
/// with the code under test.
struct Cloud {
    child: Child,
    base_url: String,
}

impl Drop for Cloud {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// ONE BUILD, SERIALISED, PER TEST BINARY.
///
/// Every test here needs `cmd/api`, and cargo runs them on parallel threads
/// of one process. Building per test meant two `go build -o` invocations
/// racing for the same output path while a third thread was spawning it --
/// on Windows that is `os error 32`, the linker's sharing violation, and it
/// fires only when the Go sources actually changed since the last run. The
/// falsification pass for criterion 6 edits Go, so the failure it produced
/// was this race and not the assertion under test: a test that cannot be
/// falsified on demand is not evidence of anything.
///
/// `OnceLock::get_or_init` blocks every other thread until the first one
/// finishes, so the build happens exactly once and every test afterwards
/// spawns an executable nobody is still writing. Concurrent *reads* of one
/// exe are fine on Windows; concurrent writes are what was never allowed.
static API_BINARY: OnceLock<PathBuf> = OnceLock::new();

fn api_binary(root: &Path) -> PathBuf {
    API_BINARY
        .get_or_init(|| {
            let exe = root.join("target").join(if cfg!(windows) {
                "holler-api-e2e.exe"
            } else {
                "holler-api-e2e"
            });
            std::fs::create_dir_all(root.join("target")).ok();
            run(
                "go build ./cmd/api",
                Command::new("go")
                    .args(["build", "-o"])
                    .arg(&exe)
                    .arg("./cmd/api")
                    .current_dir(root.join("backend")),
            );
            exe
        })
        .clone()
}

fn start_cloud(root: &Path, db_url: &str) -> Cloud {
    let exe = api_binary(root);

    let port = free_port();
    let child = Command::new(&exe)
        .env("DATABASE_URL", db_url)
        .env("PORT", port.to_string())
        .env("TOKEN_SIGNING_KEY", "cloud-e2e-signing-key-not-for-prod")
        .env(
            "CONTRACTS_DIR",
            root.join("packages").join("contracts").join("postgres"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning cmd/api");

    let cloud = Cloud {
        child,
        base_url: format!("http://127.0.0.1:{port}"),
    };
    wait_for_health(&cloud);
    cloud
}

fn wait_for_health(cloud: &Cloud) {
    let deadline = Instant::now() + BOOT_TIMEOUT;
    let url = format!("{}/health", cloud.base_url);
    while Instant::now() < deadline {
        if ureq::get(&url)
            .timeout(Duration::from_secs(2))
            .call()
            .is_ok()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("cmd/api did not become healthy on {url} within {BOOT_TIMEOUT:?}");
}

// -------------------------------------------------------------- HTTP -------

fn post_json(url: &str, headers: &[(&str, &str)], body: &Value) -> (u16, Value) {
    let mut req = ureq::post(url).timeout(Duration::from_secs(20));
    for (k, v) in headers {
        req = req.set(k, v);
    }
    match req.send_json(body.clone()) {
        Ok(resp) => {
            let status = resp.status();
            let value = resp.into_json::<Value>().unwrap_or(Value::Null);
            (status, value)
        }
        Err(ureq::Error::Status(status, resp)) => {
            let mut text = String::new();
            let _ = resp.into_reader().read_to_string(&mut text);
            panic!("POST {url} -> {status}: {text}");
        }
        Err(e) => panic!("POST {url}: {e}"),
    }
}

fn login(base_url: &str, seed: &Seed) -> String {
    let (_, body) = post_json(
        &format!("{base_url}/auth/login"),
        &[("X-Tenant-ID", seed.tenant_id.as_str())],
        &json!({
            "email": seed.email,
            "password": seed.password,
            "outlet_id": seed.outlet_id,
        }),
    );
    body["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("login returned no access_token: {body}"))
        .to_string()
}

/// Enrolls a device the way an outlet actually gets one (ADR-017), and keeps
/// the plaintext token, which is returned exactly once and never again.
fn enroll_device(base_url: &str, seed: &Seed, access_token: &str) -> (String, String) {
    let (_, body) = post_json(
        &format!("{base_url}/devices/enroll"),
        &[
            ("X-Tenant-ID", seed.tenant_id.as_str()),
            ("Authorization", &format!("Bearer {access_token}")),
        ],
        &json!({
            "outlet_id": seed.outlet_id,
            "kind": "POS",
            // Unique per call: enrollment conflicts on (tenant, outlet, name)
            // and answers 409 "rotate its credential instead". A fixed name
            // would make the FIRST run of this file green and every run after
            // it red -- the same clean-database-only trap the sync-config
            // integration test was in (docs/retro.md 2026-08-23).
            "name": format!("cloud-e2e {}", new_id()),
            "label": "criterion 6",
        }),
    );
    let device_id = body["device_id"].as_str().expect("device_id").to_string();
    let token = body["token"].as_str().expect("token").to_string();
    (device_id, token)
}

fn create_inventory_item(base_url: &str, seed: &Seed, access_token: &str, id: &str, sku: &str) {
    post_json(
        &format!("{base_url}/inventory/items"),
        &[
            ("X-Tenant-ID", seed.tenant_id.as_str()),
            ("Authorization", &format!("Bearer {access_token}")),
        ],
        &json!({
            "id": id,
            "outlet_id": seed.outlet_id,
            "sku": sku,
            "name": "Onion",
            "dimension": "MASS",
            "is_active": true,
        }),
    );
}

// --------------------------------------------------------------- the edge --

fn open_edge_db(dir: &Path, seed: &Seed, item_id: &str, sku: &str) -> Db {
    let db = Db::open_in_memory_for_tests().expect("opening edge db");
    let _ = dir;

    repo::upsert_outlet(
        db.connection(),
        &model::Outlet {
            id: seed.outlet_id.clone(),
            brand_id: new_id(),
            name: "Cloud E2E Outlet".to_string(),
            timezone: OUTLET_TZ.to_string(),
            config_version: 1,
            created_at: "2026-08-23T00:00:00Z".to_string(),
            updated_at: "2026-08-23T00:00:00Z".to_string(),
        },
    )
    .expect("seeding outlet");

    repo::upsert_inventory_item(
        db.connection(),
        &model::InventoryItem {
            id: item_id.to_string(),
            outlet_id: seed.outlet_id.clone(),
            sku: sku.to_string(),
            name: "Onion".to_string(),
            category: Some("Produce".to_string()),
            dimension: "MASS".to_string(),
            reorder_level_micro: Some(25_000_000),
            par_level_micro: None,
            storage_location: None,
            is_active: true,
            yield_factor_ppm: 1_000_000,
            config_version: 1,
        },
    )
    .expect("seeding inventory item");

    db
}

// ----------------------------------------------------- the payload shapes --

/// The wire shape of a `stock_ledger_entry`, rebuilt from the edge's own row.
///
/// Deliberately hand-written to mirror `ranged.rs::ledger_entry_payload`: if
/// the pump's serialiser and this one disagree, that disagreement is a finding
/// about the pump, and deriving both from one function would hide it.
fn edge_row_as_wire(e: &model::StockLedgerEntry) -> Value {
    json!({
        "id": e.id,
        "outlet_id": e.outlet_id,
        "entry_seq": e.entry_seq,
        "inventory_item_id": e.inventory_item_id,
        "inventory_item_name": e.inventory_item_name,
        "dimension": e.dimension,
        "entry_type": e.entry_type,
        "origin": e.origin,
        "quantity_applied_micro": e.quantity_applied_micro,
        "recipe_id": e.recipe_id,
        "recipe_version": e.recipe_version,
        "recipe_name": e.recipe_name,
        "source_order_id": e.source_order_id,
        "source_order_item_id": e.source_order_item_id,
        "reason_code": e.reason_code,
        "source_stock_count_id": e.source_stock_count_id,
        "note": e.note,
        "occurred_at": e.occurred_at,
        "business_date": e.business_date,
        "created_by_user_id": e.created_by_user_id,
        "modifier_delta_id": e.modifier_delta_id,
        "modifier_name": e.modifier_name,
        "modifier_delta_version": e.modifier_delta_version,
        "unit_cost_paise": e.unit_cost_paise,
        // Contracts 0.6.0. These three lagged here after `ranged.rs` gained
        // them (1e6455b) and THAT IS THE MIRROR EARNING ITS KEEP -- the job of
        // a hand-written twin is to disagree out loud. What it must never do
        // is disagree by staying null on both sides, which is why the fixture
        // below earns a real receipt, return and dispatch.
        "source_grn_id": e.source_grn_id,
        "source_purchase_return_id": e.source_purchase_return_id,
        "source_stock_transfer_out_id": e.source_stock_transfer_out_id,
        "schema_version": 1,
    })
}

/// The same shape, rebuilt by RE-SERIALISING what PostgreSQL actually stored.
///
/// Every column is projected as text and retyped here, so the comparison is
/// against the stored bytes rather than against a driver's opinion of them.
/// Timestamps are rendered exactly as `scanLedgerEntryRow` renders them --
/// RFC3339 in UTC, and `business_date` as a bare date -- because that is the
/// form the cloud itself hands to any reader.
fn postgres_row_as_wire(pg: &mut PgClient, entry_id: &str) -> Value {
    let row = pg
        .query_one(
            "SELECT id::text, outlet_id::text, entry_seq::text, inventory_item_id::text,
                    inventory_item_name, dimension::text, entry_type::text, origin::text,
                    quantity_applied_micro::text, recipe_id::text, recipe_version::text,
                    recipe_name, source_order_id::text, source_order_item_id::text,
                    reason_code, note,
                    to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                    to_char(business_date, 'YYYY-MM-DD'),
                    created_by_user_id::text, modifier_delta_id::text, modifier_name,
                    modifier_delta_version::text, unit_cost_paise::text,
                    source_stock_count_id::text,
                    source_grn_id::text, source_purchase_return_id::text,
                    source_stock_transfer_out_id::text
             FROM stock_ledger_entry WHERE id::text = $1",
            &[&uuid_param(entry_id)],
        )
        .expect("the entry must be in postgres after a successful replay");

    let text = |i: usize| row.get::<_, Option<String>>(i);
    let num = |i: usize| match text(i) {
        Some(v) => Value::Number(v.parse::<i64>().expect("integer column").into()),
        None => Value::Null,
    };
    let str_or_null = |i: usize| match text(i) {
        Some(v) => Value::String(v),
        None => Value::Null,
    };

    json!({
        "id": str_or_null(0),
        "outlet_id": str_or_null(1),
        "entry_seq": num(2),
        "inventory_item_id": str_or_null(3),
        "inventory_item_name": str_or_null(4),
        "dimension": str_or_null(5),
        "entry_type": str_or_null(6),
        "origin": str_or_null(7),
        "quantity_applied_micro": num(8),
        "recipe_id": str_or_null(9),
        "recipe_version": num(10),
        "recipe_name": str_or_null(11),
        "source_order_id": str_or_null(12),
        "source_order_item_id": str_or_null(13),
        "reason_code": str_or_null(14),
        // Contracts 0.5.9. Until then this column was read out here and
        // deliberately NOT compared, because the cloud dropped it on decode
        // and stored NULL for every row.
        "source_stock_count_id": str_or_null(23),
        // Contracts 0.6.0, and the same reasoning one milestone on: these were
        // NULL in postgres for every replayed procurement movement while the
        // edge's own copy was correct, because the payload never carried them.
        "source_grn_id": str_or_null(24),
        "source_purchase_return_id": str_or_null(25),
        "source_stock_transfer_out_id": str_or_null(26),
        "note": str_or_null(15),
        "occurred_at": str_or_null(16),
        "business_date": str_or_null(17),
        "created_by_user_id": str_or_null(18),
        "modifier_delta_id": str_or_null(19),
        "modifier_name": str_or_null(20),
        "modifier_delta_version": num(21),
        "unit_cost_paise": num(22),
        // Not a stored column. The wire carries it; the row does not have it
        // to lose, so it is restated rather than read back.
        "schema_version": 1,
    })
}

// ------------------------------------------------------------- the test ----

#[test]
fn a_ledger_entry_created_at_the_edge_replays_to_the_real_cloud_and_reads_back_identically() {
    let root = repo_root();
    // Declaration order IS teardown order, reversed: `pg` closes, then the
    // backend is killed, then the database is dropped. Any other order leaves
    // a connection open against a database something is trying to drop.
    let temp = TempDatabase::create(&require_database_url());
    let db_url = temp.url.clone();

    // --- cloud: schema, seed, credentials --------------------------------
    let cloud = start_cloud(&root, &db_url); // migrates on startup
    let mut pg = PgClient::connect(&db_url, NoTls).expect("connecting to the test database");
    let seed = devseed(&root, &db_url);
    grant_permissions(&mut pg, &seed.tenant_id);

    let access_token = login(&cloud.base_url, &seed);
    let (device_id, device_token) = enroll_device(&cloud.base_url, &seed, &access_token);

    // Both the id AND the sku are minted per run: the create route conflicts
    // on (outlet, sku), so a fixed sku is green once and 409 forever after.
    let item_id = new_id();
    let sku = format!("INV-ONION-{item_id}");
    create_inventory_item(&cloud.base_url, &seed, &access_token, &item_id, &sku);

    // --- edge: earn a ledger entry through the shipping API ---------------
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut db = open_edge_db(tmp.path(), &seed, &item_id, &sku);

    let created = db
        .record_wastage(model::NewWastageEntry {
            outlet_id: seed.outlet_id.clone(),
            inventory_item_id: item_id.clone(),
            quantity_micro: 2_500_000,
            reason_code: "SPOILAGE".to_string(),
            note: Some("criterion 6".to_string()),
            occurred_at: "2026-08-23T10:15:00Z".to_string(),
            created_by_user_id: None,
        })
        .expect("record_wastage");

    assert_eq!(
        created.entry_seq, 1,
        "entry_seq is 1-based (contracts 0.5.8): a 0-based counter skips every \
         outlet's first entry, permanently and silently"
    );

    // --- a SECOND entry, carrying a provenance group the first cannot ------
    //
    // A wastage row leaves every count-provenance field null, and a null
    // round-trips perfectly through a field the cloud does not declare. That
    // is exactly how `source_stock_count_id` crossed this seam unnoticed for
    // two milestones WHILE THIS TEST WAS GREEN: the fixture never populated
    // it, so the comparison could not tell "carried" from "absent".
    //
    // So the fixture earns a COUNT_ADJUSTMENT through the shipping count API
    // too. Any provenance group added later needs its own entry here for the
    // same reason -- a fidelity test proves fidelity only for the fields its
    // fixture populates, and it stays green while it proves nothing.
    let count_id = new_id();
    db.open_stock_count(model::NewStockCount {
        id: count_id.clone(),
        outlet_id: seed.outlet_id.clone(),
        started_at: "2026-08-23T21:00:00Z".to_string(),
        counted_by_user_id: None,
        note: None,
    })
    .expect("open_stock_count");

    // Counting zero against a shelf the ledger believes holds -2.5 g is a
    // non-zero variance, which is what makes completion post an adjustment at
    // all. `expected_quantity_micro` is snapshotted by the edge, never
    // supplied from here.
    db.add_or_update_stock_count_line(
        &count_id,
        &seed.outlet_id,
        model::NewStockCountLine {
            inventory_item_id: item_id.clone(),
            counted_quantity_micro: 0,
            note: None,
        },
    )
    .expect("add_or_update_stock_count_line");

    db.complete_stock_count(&count_id, &seed.outlet_id, "2026-08-23T21:05:00Z")
        .expect("complete_stock_count");

    // --- THREE MORE ENTRIES, one per procurement provenance group ---------
    //
    // Contracts 0.6.0 put `source_grn_id`, `source_purchase_return_id` and
    // `source_stock_transfer_out_id` on the row in both stores. The payload in
    // `ranged.rs` did not carry them, so the cloud stored NULL for every
    // replayed procurement movement -- 0.5.9's defect, a third time, and the
    // rule against it was already written into ADR-019 when it happened.
    //
    // A wastage row and a count adjustment leave all three legitimately null,
    // and null == null is the shape that let `source_stock_count_id` cross
    // this seam unnoticed for two milestones WHILE THIS TEST WAS GREEN. So
    // each group earns its own entry through the shipping write path, exactly
    // as the header promises: a fidelity test proves fidelity only for the
    // fields its fixture populates.
    //
    // These ride the plain outbox, not the ranged cursor, so they are pumped
    // by `pump_procurement` below -- and they MUST land first, because
    // postgres carries a real FK from each provenance column to the document
    // it names. That ordering is not test scaffolding; it is the replay order
    // the outlet itself uses.
    let user_id: String = pg
        .query_one(
            "SELECT id::text FROM app_user WHERE email = $1::text",
            &[&seed.email],
        )
        .expect("devseed must have created a user to receive against")
        .get(0);

    // The destination of the transfer. A real outlet row, because postgres
    // puts an FK on `destination_outlet_id` and a CHECK that it is not the
    // source -- the outbound half still names a real sibling (ADR-019).
    let destination_outlet_id = new_id();
    pg.execute(
        "INSERT INTO outlet (id, brand_id, name, timezone, config_version)
         SELECT $1::text::uuid, brand_id, 'Cloud E2E Destination', timezone, 1
           FROM outlet WHERE id = $2::text::uuid",
        &[
            &uuid_param(&destination_outlet_id),
            &uuid_param(&seed.outlet_id),
        ],
    )
    .expect("seeding the transfer destination outlet");

    // The edge's own copy of the receiving user. `open_edge_db` seeds config
    // only; every operational row below is written by the code under test.
    db.connection()
        .execute(
            "INSERT INTO app_user
                (id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash,
                 is_active, permissions_json, config_version, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'Cloud E2E Receiver', 'not-a-hash', NULL,
                     1, '[]', 1, '2026-08-23T00:00:00Z')",
            rusqlite::params![user_id, seed.tenant_id, seed.outlet_id, seed.email],
        )
        .expect("seeding the edge's app_user");

    // A receipt with no PO and no supplier: the walk-in case, ACCEPTED with a
    // gap (ADR-019, "a GRN never blocks on a PO"). It posts the PURCHASE entry
    // that carries `source_grn_id` -- and, because M5 put a cost on the ledger,
    // the first entry in this fixture with a non-null `unit_cost_paise`.
    let grn_id = new_id();
    db.record_goods_receipt_with_outbox(
        model::NewGoodsReceiptNote {
            id: grn_id.clone(),
            outlet_id: seed.outlet_id.clone(),
            purchase_order_id: None,
            supplier_id: None,
            delivery_note_ref: Some("DN-CRIT6".to_string()),
            received_at: "2026-08-23T11:00:00Z".to_string(),
            received_by_user_id: user_id.clone(),
            notes: None,
            lines: vec![model::NewGrnLine {
                inventory_item_id: item_id.clone(),
                entered_purchase_unit: "kg".to_string(),
                entered_quantity_micro: 4_000_000,
                // The author's own declaration, never read off the item: if a
                // write path auto-filled this the comparison becomes x == x
                // and the guard can never fire (contracts 0.5.2).
                quantity_dimension: "MASS".to_string(),
                purchase_price_paise: 3_500,
                batch_code: None,
                expiry_date: None,
                purchase_order_line_id: None,
            }],
        },
        &model::ProcurementOutboxMeta {
            outbox_id: new_id(),
            occurred_at: "2026-08-23T11:00:00Z".to_string(),
        },
    )
    .expect("record_goods_receipt_with_outbox");

    // A return against that receipt: posts RETURN_TO_VENDOR carrying
    // `source_purchase_return_id`. `unit_cost_paise: None` values it at the
    // weighted average the ledger already holds, never a guess and never zero.
    let return_id = new_id();
    db.record_purchase_return_with_outbox(
        model::NewPurchaseReturn {
            id: return_id.clone(),
            outlet_id: seed.outlet_id.clone(),
            supplier_id: None,
            grn_id: Some(grn_id.clone()),
            return_number: format!("RTV-{return_id}"),
            reason: "DAMAGED".to_string(),
            returned_at: "2026-08-23T12:00:00Z".to_string(),
            returned_by_user_id: user_id.clone(),
            notes: None,
            lines: vec![model::NewPurchaseReturnLine {
                inventory_item_id: item_id.clone(),
                grn_line_id: None,
                entered_purchase_unit: "kg".to_string(),
                entered_quantity_micro: 1_000_000,
                quantity_dimension: "MASS".to_string(),
                unit_cost_paise: None,
            }],
        },
        &model::ProcurementOutboxMeta {
            outbox_id: new_id(),
            occurred_at: "2026-08-23T12:00:00Z".to_string(),
        },
    )
    .expect("record_purchase_return_with_outbox");

    // The outbound half of a transfer: posts TRANSFER_OUT carrying
    // `source_stock_transfer_out_id`. No TRANSFER_IN is written anywhere --
    // that half spans a second edge database and is M8.
    let transfer_id = new_id();
    db.record_stock_transfer_out_with_outbox(
        model::NewStockTransferOut {
            id: transfer_id.clone(),
            outlet_id: seed.outlet_id.clone(),
            destination_outlet_id: destination_outlet_id.clone(),
            transfer_number: format!("TRF-{transfer_id}"),
            dispatched_at: "2026-08-23T13:00:00Z".to_string(),
            dispatched_by_user_id: user_id.clone(),
            notes: None,
            lines: vec![model::NewStockTransferLine {
                inventory_item_id: item_id.clone(),
                base_quantity_micro: 500_000,
                quantity_dimension: "MASS".to_string(),
                unit_cost_paise: None,
            }],
        },
        &model::ProcurementOutboxMeta {
            outbox_id: new_id(),
            occurred_at: "2026-08-23T13:00:00Z".to_string(),
        },
    )
    .expect("record_stock_transfer_out_with_outbox");

    // Read back through the SAME call the pump sends from, so what is compared
    // is what was sent rather than a second opinion about it.
    let edge_entries = repo::list_ledger_entries_after(db.connection(), &seed.outlet_id, 0, 100)
        .expect("listing the edge's ledger entries");
    assert_eq!(
        edge_entries.len(),
        5,
        "the fixture is one wastage entry, one count-sourced adjustment, and \
         one entry per procurement provenance group (receipt, return, dispatch)"
    );

    // EVERY provenance column must be POPULATED on the entry that owns it.
    // Without these four checks the whole-object comparisons below can pass on
    // null agreeing with null, which is precisely how this test stayed green
    // through contracts 0.5.9 -- and then again through 0.6.0.
    assert_eq!(
        edge_entries[1].source_stock_count_id.as_deref(),
        Some(count_id.as_str()),
        "the edge must stamp the adjustment with the count that produced it (contracts 0.5.5): if this is null, the comparison below is back to proving nothing about this field"
    );
    assert_eq!(
        edge_entries[2].source_grn_id.as_deref(),
        Some(grn_id.as_str()),
        "the PURCHASE entry must name the receipt that produced it (contracts 0.6.0)"
    );
    assert_eq!(
        edge_entries[3].source_purchase_return_id.as_deref(),
        Some(return_id.as_str()),
        "the RETURN_TO_VENDOR entry must name the return that produced it (contracts 0.6.0)"
    );
    assert_eq!(
        edge_entries[4].source_stock_transfer_out_id.as_deref(),
        Some(transfer_id.as_str()),
        "the TRANSFER_OUT entry must name the dispatch that produced it (contracts 0.6.0)"
    );
    assert!(
        edge_entries[2].unit_cost_paise.is_some(),
        "M5 puts a cost on the ledger at receipt: an uncosted PURCHASE entry \
         would make the comparison below prove nothing about unit_cost_paise either"
    );

    // --- replay over the real seam ---------------------------------------
    let worker = SyncWorker::new(WorkerConfig {
        tenant_id: seed.tenant_id.clone(),
        outlet_id: seed.outlet_id.clone(),
        device_id,
        base_url: cloud.base_url.clone(),
        device_token,
    });

    // The procurement DOCUMENTS first. Postgres puts a real FK from each
    // provenance column to the document it names, so a ledger entry whose
    // `source_grn_id` points at a receipt the cloud has not seen cannot
    // insert. This is the outlet's own replay order, not an ordering invented
    // for the test.
    let procurement = worker
        .pump_procurement(&mut db, 100)
        .expect("pumping procurement documents");
    assert!(
        procurement.stopped.is_none()
            && procurement.blocked.is_empty()
            && procurement.over_budget.is_empty()
            && procurement.refused_locally.is_empty(),
        "the procurement documents must all land before the ledger references them: \
         stopped={:?} blocked={:?} over_budget={:?} refused_locally={:?}",
        procurement.stopped,
        procurement.blocked,
        procurement.over_budget,
        procurement.refused_locally,
    );

    let report = worker
        .pump_ranged_streams(&mut db, 100)
        .expect("pumping ranged streams");

    assert!(
        report.stopped.is_none() && report.blocked.is_empty(),
        "replay did not complete: stopped={:?} blocked={:?}",
        report.stopped,
        report.blocked
    );
    assert_eq!(
        report.ledger_acked,
        vec![1, 2, 3, 4, 5],
        "the cloud must acknowledge exactly the marks the edge minted"
    );

    // --- read back, twice per entry, whole-object -------------------------
    for (i, entry) in edge_entries.iter().enumerate() {
        let sent = edge_row_as_wire(entry);
        let seq = entry.entry_seq;

        let (acked_seq, echo) = report
            .acked_echo
            .get(i)
            .expect("an accepted entry must carry the cloud's echo of it");
        assert_eq!(*acked_seq, seq);
        assert_identical(
            &format!("201 echo, entry_seq {seq} (wire fidelity, through Go's types)"),
            &sent,
            echo,
        );

        let stored = postgres_row_as_wire(&mut pg, &entry.id);
        assert_identical(
            &format!("postgres row, entry_seq {seq} (storage fidelity)"),
            &sent,
            &stored,
        );
    }

    // --- the deferred columns stay inert (ADR-018 §8) ---------------------
    let unit_cost: Option<String> = pg
        .query_one(
            "SELECT unit_cost_paise::text FROM stock_ledger_entry WHERE id::text = $1",
            &[&uuid_param(&created.id)],
        )
        .expect("reading unit_cost_paise")
        .get(0);
    assert_eq!(
        unit_cost, None,
        "unit_cost_paise must arrive NULL and stay NULL: costing is M5, and a \
         zero would be a costed entry worth nothing rather than an uncosted one"
    );

    let yield_ppm: i32 = pg
        .query_one(
            "SELECT yield_factor_ppm FROM inventory_item WHERE id::text = $1",
            &[&uuid_param(&item_id)],
        )
        .expect("reading yield_factor_ppm")
        .get(0);
    assert_eq!(
        yield_ppm, 1_000_000,
        "yield_factor_ppm must be the identity in M4 (ADR-018 §8): anything else \
         silently rescales every deduction the moment M5 starts reading it"
    );
}
