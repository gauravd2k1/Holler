//! Development/acceptance transport: writes the real ESC/POS byte stream to
//! a file instead of a device.
//!
//! **This is not a fourth `connection_kind`, and adding it was deliberately
//! not a contract change.** `printer.connection_kind` stays frozen at the
//! three real kinds (`transport/mod.rs`'s own doc comment: "adding a
//! transport is a contract change"). This sink is selected by an
//! *environment* setting at the transport boundary only —
//! `HOLLER_PRINTER_FILE_SINK_DIR`. Unset, which is every production install,
//! `build_transport` behaves exactly as it did before this file existed:
//! nothing in the config, schema, sync bundle or wire format knows this
//! transport is here.
//!
//! WHY IT EXISTS. The bytes a bill is made of can be verified without a
//! thermal printer attached; whether a physical 58/80mm printer accepts them
//! cannot. Pointing `PathTransport` at an ordinary file gets close, and is
//! what the e2e harness does, but it is subtly wrong as an observation tool:
//! that transport opens a device path without create or truncate (correct for
//! a COM port, which has no file offset), so a second, shorter bill leaves
//! the tail of the first behind and the file you read is not the bill you
//! printed. This sink writes each job to its own file, so what you open is
//! exactly one print.
//!
//! WHAT IT PROVES AND DOES NOT. The bytes here are produced by the same
//! `template::render_invoice`/`render_kot` and travel the same
//! `spool` -> `attempt_print` -> `PrinterTransport::send` path as a real
//! print; only the final `write` lands somewhere inspectable. It therefore
//! establishes that the render ran, that the spool transitioned the job, and
//! what the operator would have received. It establishes **nothing** about
//! real device I/O: no ESC/POS dialect quirks, no paper width behaviour, no
//! cutter, no codepage, no USB/serial timing. Those need hardware.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::error::{PrinterError, PrinterResult};
use crate::transport::PrinterTransport;

/// Environment variable naming the directory each print is written into.
/// Absent => this transport is never constructed.
pub const FILE_SINK_DIR_ENV: &str = "HOLLER_PRINTER_FILE_SINK_DIR";

pub struct FileSinkTransport {
    dir: PathBuf,
    printer_id: String,
    printer_name: String,
    /// The stem (no extension) `send` most recently wrote, so a following
    /// [`PrinterTransport::send_html_companion`] call lands beside the exact
    /// `.escpos` file it belongs with rather than minting a fresh timestamp
    /// that would leave the pair looking unrelated.
    last_stem: Option<String>,
}

impl FileSinkTransport {
    pub fn new(dir: PathBuf, printer_id: String, printer_name: String) -> Self {
        Self {
            dir,
            printer_id,
            printer_name,
            last_stem: None,
        }
    }

    /// `printer.name` reduced to a filename-safe token. Anything that is not
    /// alphanumeric collapses to `-`, so a printer called "Front Counter /
    /// Bill" cannot create a subdirectory or escape `dir`.
    fn safe_name(&self) -> String {
        let token: String = self
            .printer_name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let trimmed = token.trim_matches('-').to_string();
        if trimmed.is_empty() {
            self.printer_id.clone()
        } else {
            trimmed
        }
    }
}

/// Renders the ESC/POS stream as the text an operator would see on paper:
/// every escape sequence removed, every printable byte kept. Written
/// alongside the raw file purely so a human can read the bill without an
/// ESC/POS decoder — the `.escpos` file remains the artefact of record, and
/// nothing in the product ever reads this back.
///
/// Handles the sequences `EscPosBuilder` actually emits: `ESC @`, `ESC a n`,
/// `ESC E n`, `GS ! n`, `GS V m`. Any other escape is skipped conservatively
/// (the escape byte and one following byte), which can only ever affect this
/// human-readable companion, never the bytes sent.
fn to_readable_text(bytes: &[u8]) -> String {
    const ESC: u8 = 0x1B;
    const GS: u8 = 0x1D;
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            ESC => {
                // ESC @ takes no parameter; every other ESC sequence this
                // codebase emits takes exactly one.
                if i + 1 < bytes.len() && bytes[i + 1] == b'@' {
                    i += 2;
                } else {
                    i += 3;
                }
            }
            GS => {
                // GS ! n and GS V m are both two bytes plus the parameter.
                i += 3;
            }
            b'\n' => {
                out.push('\n');
                i += 1;
            }
            b if (0x20..0x7F).contains(&b) => {
                out.push(b as char);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

impl PrinterTransport for FileSinkTransport {
    fn send(&mut self, bytes: &[u8]) -> PrinterResult<()> {
        let transport_err = |message: String| PrinterError::Transport {
            printer_id: self.printer_id.clone(),
            address: self.dir.to_string_lossy().into_owned(),
            message,
        };

        fs::create_dir_all(&self.dir)
            .map_err(|e| transport_err(format!("create sink dir: {e}")))?;

        // One file per print. The timestamp is the ordering key a human
        // reads; the nanosecond component keeps two prints in the same
        // millisecond from colliding, which a split bill printing its parts
        // back to back will genuinely do.
        let now = chrono::Utc::now();
        let stem = format!("{}-{}", now.format("%Y%m%dT%H%M%S%.9f"), self.safe_name());

        let raw_path = self.dir.join(format!("{stem}.escpos"));
        let mut file = fs::File::create(&raw_path)
            .map_err(|e| transport_err(format!("create {}: {e}", raw_path.display())))?;
        file.write_all(bytes)
            .map_err(|e| transport_err(format!("write {}: {e}", raw_path.display())))?;
        file.flush()
            .map_err(|e| transport_err(format!("flush {}: {e}", raw_path.display())))?;

        // Best-effort companion. A failure to write the human-readable copy
        // must never fail the print: the bytes are already out, and a real
        // printer would have printed them.
        let text_path = self.dir.join(format!("{stem}.txt"));
        let _ = fs::write(&text_path, to_readable_text(bytes));

        self.last_stem = Some(stem);

        // The one line that makes a file-backed run observable in the
        // console. Deliberately on stdout, not behind a log level, because
        // an acceptance run's whole purpose is watching this happen.
        println!(
            "holler-printer: FILE SINK wrote {} bytes for printer {} ({}) -> {}",
            bytes.len(),
            self.printer_name,
            self.printer_id,
            raw_path.display()
        );
        Ok(())
    }

    /// Writes `<same-stem-as-the-preceding-send>.html`, renders it to
    /// `<same-stem>.pdf`, and opens **one** of them with the OS default
    /// handler — the rendered bill a demo shows on screen (T9), now as the
    /// PDF demo step 2 actually asks for.
    ///
    /// Best-effort in every half, and the order matters: a write failure, a
    /// missing browser, a render failure or a failure to launch a viewer is
    /// logged and never propagated. That is the `.txt` companion's own rule —
    /// **the bytes are already out and the bill is already committed by the
    /// time this runs**, so nothing here may turn a successful print into a
    /// failed one. When the PDF cannot be produced the HTML is opened
    /// instead, which is exactly the behaviour this method had before the PDF
    /// existed.
    ///
    /// Requires a preceding [`PrinterTransport::send`] on this instance
    /// (`last_stem` set); if none happened, the call is a silent no-op rather
    /// than inventing an unrelated stem.
    fn send_html_companion(&mut self, html: &str) -> PrinterResult<()> {
        let Some(stem) = self.last_stem.clone() else {
            return Ok(());
        };
        let html_path = self.dir.join(format!("{stem}.html"));
        if let Err(e) = fs::write(&html_path, html) {
            eprintln!(
                "holler-printer: FILE SINK failed to write html companion {}: {e}",
                html_path.display()
            );
            return Ok(());
        }
        println!(
            "holler-printer: FILE SINK wrote receipt {}",
            html_path.display()
        );

        // THE PDF IS A RENDERING OF THE FILE JUST WRITTEN, NOT A THIRD
        // RENDERER. `render_invoice_html` stays the only HTML producer and is
        // untouched, so the equivalence test binding that HTML line-for-line to
        // the ESC/POS bytes still covers the PDF's content transitively. A
        // separate PDF template would be a third description of one bill, which
        // is the defect the HTML/bytes equivalence test exists to prevent.
        let pdf_path = self.dir.join(format!("{stem}.pdf"));
        let opened = match render_pdf_from_html(&html_path, &pdf_path) {
            Ok(()) => {
                println!(
                    "holler-printer: FILE SINK wrote receipt PDF {}",
                    pdf_path.display()
                );
                &pdf_path
            }
            Err(e) => {
                eprintln!(
                    "holler-printer: FILE SINK could not render {}: {e}. Opening the HTML \
                     instead; the bill itself is unaffected.",
                    pdf_path.display()
                );
                &html_path
            }
        };

        if std::env::var_os(SUPPRESS_OPEN_ENV).is_some() {
            // Announced, never silent. This variable removes the thing demo
            // step 2 actually shows -- the bill appearing on screen -- so a
            // machine that has it set must say so every time rather than look
            // like a build where opening quietly stopped working.
            println!(
                "holler-printer: FILE SINK did NOT open {} because {SUPPRESS_OPEN_ENV} is set",
                opened.display()
            );
            return Ok(());
        }
        if let Err(e) = open_with_os_default(opened) {
            eprintln!(
                "holler-printer: FILE SINK could not open {} in a viewer: {e}",
                opened.display()
            );
        }
        Ok(())
    }
}

/// Set to anything to stop the file sink launching a viewer for the receipt it
/// just wrote. **It exists so the fallback path can be tested without a browser
/// window opening on the machine running the tests**, and it is deliberately
/// loud: every suppressed open prints a line saying so. Unset -- which is every
/// demo and every acceptance run -- changes nothing.
pub const SUPPRESS_OPEN_ENV: &str = "HOLLER_RECEIPT_SUPPRESS_OPEN";

/// Environment override naming the Chromium-family executable used to render
/// the receipt PDF. Set it when Edge is somewhere [`find_pdf_browser`] does not
/// look, or to pin a specific build.
pub const PDF_BROWSER_ENV: &str = "HOLLER_RECEIPT_PDF_BROWSER";

/// How long the headless render may take before it is killed. A receipt is one
/// small local page with no network fetches, so seconds is generous — and the
/// deadline exists because a browser that hangs must not hold a till's print
/// worker, which is a far worse outcome than a missing PDF.
const PDF_RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Locates a Chromium-family browser to render with, or `None`.
///
/// **NO NEW DEPENDENCY, IN EITHER SENSE.** No crate is added to the edge
/// binary, and nothing is installed on the machine: this reaches for the Edge
/// that ships with Windows, and the whole path lives behind the file-sink
/// transport, which is selected by an environment variable and is never
/// constructed on a real install (ADR-013 — the outlet runs one native
/// executable and gains nothing here). A machine without a browser simply gets
/// the HTML, as it did before.
fn find_pdf_browser() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var(PDF_BROWSER_ENV) {
        let path = PathBuf::from(explicit.trim());
        // An override that names a missing file is a CONFIGURATION ERROR and is
        // reported as one. Falling through to the default search would hide it,
        // and the operator would spend the difference wondering why their
        // pinned build was ignored.
        if path.is_file() {
            return Some(path);
        }
        eprintln!(
            "holler-printer: {PDF_BROWSER_ENV}={} does not name a file; falling back to the \
             default search",
            path.display()
        );
    }

    // Edge first: it is present on every supported Windows install, so it is
    // the one that does not depend on what the operator happens to have.
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Renders `html_path` to `pdf_path` with headless Chromium.
///
/// THE RESULT IS VERIFIED, NOT ASSUMED. `--print-to-pdf` can exit 0 having
/// written nothing, so this waits for the process, then requires the file to
/// exist, to be non-empty, and to start with `%PDF-`. "The command returned
/// success" and "there is a PDF on disk" are different claims, and only the
/// second one is worth opening in a viewer.
fn render_pdf_from_html(
    html_path: &std::path::Path,
    pdf_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    let browser = find_pdf_browser().ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!(
                "no Chromium-family browser found for PDF rendering (looked for Edge and \
                 Chrome; set {PDF_BROWSER_ENV} to override)"
            ),
        )
    })?;

    // ABSOLUTE PATHS, BOTH. The sink directory is routinely relative
    // (`.dev-prints`), and the browser is a different process with a different
    // working directory -- a relative path here renders nothing and reports
    // nothing useful about why.
    let html_abs = fs::canonicalize(html_path)?;
    let pdf_abs = if pdf_path.is_absolute() {
        pdf_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(pdf_path)
    };
    // `canonicalize` yields a \\?\ extended-length path on Windows, which
    // Chromium's URL parser does not accept as a positional argument.
    let html_arg = strip_extended_length_prefix(&html_abs);

    // A DEDICATED PROFILE DIRECTORY IS NOT OPTIONAL. Headless Chromium sharing
    // the default profile with the browser the operator already has open
    // either fails to acquire the profile lock or quietly hands the work to
    // that running instance -- which then does not write the PDF. Either way
    // the bill does not appear, at a demo, for a reason nothing on screen
    // explains.
    let profile_dir = std::env::temp_dir().join("holler-receipt-pdf-profile");

    let mut child = std::process::Command::new(&browser)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-pdf-header-footer")
        .arg(format!("--print-to-pdf={}", pdf_abs.display()))
        .arg(&html_arg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    // std has no wait-with-timeout, and a bare `wait()` would let a wedged
    // browser hold the print worker open indefinitely.
    let deadline = std::time::Instant::now() + PDF_RENDER_TIMEOUT;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::new(
                    ErrorKind::TimedOut,
                    format!(
                        "{} did not finish rendering within {}s",
                        browser.display(),
                        PDF_RENDER_TIMEOUT.as_secs()
                    ),
                ));
            }
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    };

    let produced = fs::read(&pdf_abs).unwrap_or_default();
    if !produced.starts_with(b"%PDF-") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "{} exited {} but {} is {} -- no PDF was produced",
                browser.display(),
                status,
                pdf_abs.display(),
                if produced.is_empty() {
                    "missing or empty".to_string()
                } else {
                    format!("{} bytes that do not begin %PDF-", produced.len())
                }
            ),
        ));
    }
    Ok(())
}

/// Drops the `\\?\` extended-length prefix `fs::canonicalize` adds on Windows.
/// Chromium rejects such a path as a URL; every other consumer accepts the
/// plain form equally.
fn strip_extended_length_prefix(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// Opens `path` with whatever the OS registers for its extension —
/// `cmd /C start` on Windows, the only platform this ships to (ADR-013).
/// Never returns a "printer" error: the caller treats any failure here as
/// non-fatal logging, never a reason to fail a print that already
/// succeeded.
#[cfg(target_os = "windows")]
fn open_with_os_default(path: &std::path::Path) -> std::io::Result<()> {
    // `start`'s first quoted argument is a window title, not the target —
    // pass an explicit empty title so a path containing spaces is not
    // misread as one.
    std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(target_os = "windows"))]
fn open_with_os_default(_path: &std::path::Path) -> std::io::Result<()> {
    // ADR-013: the outlet target is Windows-only. A non-Windows dev run
    // (e.g. `cargo test` on a contributor's Mac) simply does not get the
    // open-on-print affordance; it is not a supported runtime for this
    // crate's binaries.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_print_lands_in_its_own_file_with_the_exact_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut transport = FileSinkTransport::new(
            dir.path().to_path_buf(),
            "printer-1".to_string(),
            "Front Counter".to_string(),
        );

        transport.send(b"FIRST BILL").expect("first send");
        transport.send(b"SECOND").expect("second send");

        let raw: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "escpos"))
            .collect();
        assert_eq!(raw.len(), 2, "each print must land in its own file");

        // The second, shorter print must be exactly itself — the failure
        // mode that makes PathTransport unsuitable as an observation tool
        // (no truncate, so the tail of a longer previous print survives).
        let mut contents: Vec<Vec<u8>> = raw.iter().map(|e| fs::read(e.path()).unwrap()).collect();
        contents.sort();
        assert_eq!(contents[0], b"FIRST BILL".to_vec());
        assert_eq!(contents[1], b"SECOND".to_vec());
    }

    #[test]
    fn a_printer_name_cannot_escape_the_sink_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut transport = FileSinkTransport::new(
            dir.path().to_path_buf(),
            "printer-1".to_string(),
            "../../evil name".to_string(),
        );
        transport.send(b"x").expect("send");

        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "escpos"))
            .collect();
        assert_eq!(entries.len(), 1, "the print must stay inside the sink dir");
    }

    // ------------------------------------------------- the receipt PDF --

    /// `PDF_BROWSER_ENV` and `SUPPRESS_OPEN_ENV` are process-global, so every
    /// assertion that depends on them lives in ONE test. Two tests setting the
    /// same variable under `cargo test`'s thread pool would pass or fail by
    /// scheduling, which is worse than no test.
    #[test]
    fn pdf_rendering_verifies_the_artefact_and_never_fails_the_print() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut transport = FileSinkTransport::new(
            dir.path().to_path_buf(),
            "printer-1".to_string(),
            "Front Counter".to_string(),
        );

        // An override naming something that is not a file must not be returned
        // as if it were a browser.
        let missing = dir.path().join("no-such-browser.exe");
        std::env::set_var(PDF_BROWSER_ENV, &missing);
        assert_ne!(
            find_pdf_browser(),
            Some(missing.clone()),
            "an override naming a missing file must fall back, not be handed on"
        );

        // A real file IS honoured, whatever it is -- discovery's job is to find
        // an executable, not to validate it.
        let decoy = dir.path().join("decoy.exe");
        fs::write(&decoy, b"not really a browser").expect("write decoy");
        std::env::set_var(PDF_BROWSER_ENV, &decoy);
        assert_eq!(
            find_pdf_browser(),
            Some(decoy.clone()),
            "an explicit override naming a real file must win"
        );

        // THE LOAD-BEARING ASSERTION. With a "browser" that cannot produce a
        // PDF, the companion must still write the HTML and must still return
        // Ok -- the ESC/POS bytes are already out and the bill is already
        // committed, so nothing here may turn a successful print into a failed
        // one.
        std::env::set_var(SUPPRESS_OPEN_ENV, "1");
        transport.send(b"BILL BYTES").expect("send");
        let result = transport.send_html_companion("<html><body>Receipt</body></html>");
        assert!(
            result.is_ok(),
            "a failed PDF render must never fail the print: {result:?}"
        );

        let stem = transport.last_stem.clone().expect("stem");
        let html_path = dir.path().join(format!("{stem}.html"));
        assert!(
            html_path.is_file(),
            "the HTML companion must survive a failed PDF render"
        );
        let pdf_path = dir.path().join(format!("{stem}.pdf"));
        assert!(
            !pdf_path.is_file(),
            "nothing may be left behind claiming to be a PDF when none was produced"
        );

        // AND THE VERIFICATION ITSELF, which needs a decoy that genuinely RUNS.
        // The first version of this assertion used the non-executable decoy
        // above and passed with the `%PDF-` check deleted -- the spawn failed
        // before the verification was ever reached, so it was green on a code
        // path it never entered. `where.exe` is a real Windows executable that,
        // handed these arguments, exits non-zero having written nothing: the
        // exact shape of "the command ran and produced no PDF".
        let real_exe = PathBuf::from(r"C:\Windows\System32\where.exe");
        if real_exe.is_file() {
            std::env::set_var(PDF_BROWSER_ENV, &real_exe);
            let err = render_pdf_from_html(&html_path, &pdf_path)
                .expect_err("a command that writes no PDF must be an error");
            let message = err.to_string();
            assert!(
                message.contains("no PDF was produced"),
                "the error must name the MISSING ARTEFACT, not merely a failed command -- an \
                 exit code is not a PDF: {message}"
            );
            assert!(
                !pdf_path.is_file(),
                "no file may be left behind claiming to be a PDF"
            );
        } else {
            eprintln!(
                "SKIPPED the artefact-verification half: {} is absent, so the '%PDF-' check was \
                 NOT exercised by this run.",
                real_exe.display()
            );
        }

        std::env::remove_var(PDF_BROWSER_ENV);
        std::env::remove_var(SUPPRESS_OPEN_ENV);
    }

    /// The positive case, because a path only ever seen failing proves only
    /// that it can fail. Runs against whatever Chromium-family browser this
    /// machine actually has; where there is none -- Linux CI -- it says so
    /// LOUDLY rather than passing quietly, because a test that silently
    /// asserts nothing is indistinguishable from one that passed.
    #[test]
    fn a_real_browser_renders_a_real_pdf() {
        let Some(browser) = find_pdf_browser() else {
            eprintln!(
                "SKIPPED a_real_browser_renders_a_real_pdf: no Chromium-family browser on this \
                 machine. THIS TEST ASSERTED NOTHING. Set {PDF_BROWSER_ENV} to run it."
            );
            return;
        };
        eprintln!("rendering with {}", browser.display());

        let dir = tempfile::tempdir().expect("tempdir");
        let html_path = dir.path().join("receipt.html");
        fs::write(
            &html_path,
            "<html><head><style>@page { size: 80mm auto; margin: 4mm; }</style></head>\
             <body><p>Shinjuku Yakitori</p><p>FY26/PNQ/001423</p></body></html>",
        )
        .expect("write html");

        let pdf_path = dir.path().join("receipt.pdf");
        render_pdf_from_html(&html_path, &pdf_path).expect("render a real pdf");

        let bytes = fs::read(&pdf_path).expect("read pdf");
        assert!(
            bytes.starts_with(b"%PDF-"),
            "the rendered file must actually be a PDF"
        );
        assert!(
            bytes.len() > 1000,
            "a one-page receipt PDF under 1KB is an empty page, not a bill: {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn readable_companion_strips_escapes_and_keeps_the_text() {
        // ESC @ (init), ESC a 1 (centre), text, GS ! 0 (size), more text.
        let bytes = b"\x1B@\x1Ba\x01Holler Cafe\n\x1D!\x00Butter Chicken\n";
        let text = to_readable_text(bytes);
        assert!(text.contains("Holler Cafe"), "got: {text}");
        assert!(text.contains("Butter Chicken"), "got: {text}");
        assert!(!text.contains('\x1B'), "escapes must not survive: {text:?}");
        assert!(!text.contains('\x1D'), "escapes must not survive: {text:?}");
    }

    // ----------------------------------- T9: HTML/ESC-POS content equivalence --

    use holler_edge_database::model::{Invoice, InvoiceLine};

    use crate::template::{render_invoice, render_invoice_html, InvoicePrintContext};

    fn fiscal_profile_json() -> String {
        serde_json::json!({
            "id": "018e5a2e-0000-7c3d-9f4e-000000000001",
            "outlet_id": "018e5a2e-0000-7c3d-9f4e-000000000002",
            "legal_name": "Holler Hospitality Pvt Ltd",
            "trade_name": "The Holler Kitchen",
            "address_line1": "12 MG Road",
            "address_line2": "Shivaji Nagar",
            "city": "Pune",
            "state_code": "27",
            "state_name": "Maharashtra",
            "pincode": "411001",
            "gstin": "27ABCDE1234F1Z5",
            "fssai_number": "10012345678901",
            "invoice_footer_text": "Thank you, visit again!",
            "effective_from": "2026-01-01T00:00:00Z",
        })
        .to_string()
    }

    fn invoice_fixture() -> Invoice {
        Invoice {
            id: "018e5a2e-3333-7c3d-9f4e-1234567890ab".to_string(),
            outlet_id: "018e5a2e-0000-7c3d-9f4e-000000000002".to_string(),
            order_id: "018e5a2e-4444-7c3d-9f4e-abcdefabcdef".to_string(),
            split_group_id: None,
            split_index: 1,
            split_count: 1,
            series_id: "018e5a2e-0000-7c3d-9f4e-000000000003".to_string(),
            invoice_number: "FY26/PNQ/001423".to_string(),
            invoice_date: "2026-08-14T12:30:00Z".to_string(),
            business_date: "2026-08-14".to_string(),
            status: "ISSUED".to_string(),
            cancelled_reason: None,
            cancelled_at: None,
            customer_name: Some("Walk-in".to_string()),
            customer_phone: None,
            customer_gstin: None,
            place_of_supply_state_code: "27".to_string(),
            subtotal_paise: 50000,
            discount_paise: 0,
            taxable_value_paise: 50000,
            cgst_paise: 1250,
            sgst_paise: 1250,
            igst_paise: 0,
            cess_paise: 0,
            round_off_paise: 0,
            grand_total_paise: 52500,
            compliance_version_id: "018e5a2e-0000-7c3d-9f4e-000000000004".to_string(),
            tax_snapshot_json: "{}".to_string(),
            fiscal_profile_json: fiscal_profile_json(),
            channel: "POS".to_string(),
            tax_liability_party: "RESTAURANT".to_string(),
            eco_operator_name: None,
            eco_operator_gstin: None,
            supply_classification: None,
            created_by_user_id: "018e5a2e-0000-7c3d-9f4e-000000000005".to_string(),
            created_at: "2026-08-14T12:30:00Z".to_string(),
            updated_at: "2026-08-14T12:30:00Z".to_string(),
            version: 1,
            sync_status: "PENDING".to_string(),
        }
    }

    fn invoice_lines_fixture() -> Vec<InvoiceLine> {
        vec![InvoiceLine {
            id: "018e5a2e-5555-7c3d-9f4e-fedcba987654".to_string(),
            invoice_id: "018e5a2e-3333-7c3d-9f4e-1234567890ab".to_string(),
            order_item_id: "018e5a2e-6666-7c3d-9f4e-000000000009".to_string(),
            line_no: 1,
            description: "Butter Chicken".to_string(),
            hsn_sac: Some("996331".to_string()),
            quantity: 2,
            unit_price_paise: 25000,
            gross_paise: 50000,
            discount_paise: 0,
            taxable_value_paise: 50000,
            tax_profile_id: "018e5a2e-0000-7c3d-9f4e-000000000006".to_string(),
            cgst_rate_bps: 250,
            cgst_paise: 1250,
            sgst_rate_bps: 250,
            sgst_paise: 1250,
            igst_rate_bps: 0,
            igst_paise: 0,
            cess_rate_bps: 0,
            cess_paise: 0,
            total_paise: 52500,
        }]
    }

    fn invoice_ctx<'a>() -> InvoicePrintContext<'a> {
        InvoicePrintContext {
            order_display_number: "#A184",
            table_label: Some("T-04"),
            payment_summary: Some("Cash"),
        }
    }

    /// Non-empty, non-rule lines from the ESC/POS byte stream via the same
    /// [`to_readable_text`] the file sink writes as `.txt` — trimmed, and
    /// with pure-dash rule lines dropped (the HTML renderer draws those as
    /// a CSS border, not as a text line, so they are not part of the
    /// content being compared).
    fn escpos_content_lines(bytes: &[u8]) -> Vec<String> {
        to_readable_text(bytes)
            .split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.chars().all(|c| c == '-'))
            .map(str::to_string)
            .collect()
    }

    /// Content of every `<div class="line">...</div>` in
    /// [`render_invoice_html`]'s output, in document order, with the five
    /// entities [`crate::template`]'s escaper produces decoded back. This is
    /// the HTML side of the equivalence check — deliberately a bare
    /// tag-stripper, not an HTML parser, since the renderer under test
    /// emits nothing more exotic than one `<div class="line">` per content
    /// line.
    fn html_content_lines(html: &str) -> Vec<String> {
        const OPEN: &str = "<div class=\"line\">";
        const CLOSE: &str = "</div>";
        let mut lines = Vec::new();
        let mut rest = html;
        while let Some(start) = rest.find(OPEN) {
            let after_open = &rest[start + OPEN.len()..];
            let end = after_open.find(CLOSE).expect("unterminated line div");
            let raw = &after_open[..end];
            let unescaped = raw
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&amp;", "&");
            lines.push(unescaped.trim().to_string());
            rest = &after_open[end + CLOSE.len()..];
        }
        lines
    }

    #[test]
    fn html_receipt_agrees_line_for_line_with_the_escpos_bytes_for_the_same_invoice() {
        let invoice = invoice_fixture();
        let lines = invoice_lines_fixture();
        let ctx = invoice_ctx();

        let bytes = render_invoice(&invoice, &lines, &ctx, 80).expect("renders escpos");
        let html = render_invoice_html(&invoice, &lines, &ctx).expect("renders html");

        let escpos_lines = escpos_content_lines(&bytes);
        let html_lines = html_content_lines(&html);

        assert!(
            !escpos_lines.is_empty(),
            "escpos extraction produced nothing"
        );
        assert_eq!(
            escpos_lines, html_lines,
            "rendered receipt must agree line-for-line with the ESC/POS bytes for the same invoice"
        );
    }
}
