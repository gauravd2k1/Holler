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
}

impl FileSinkTransport {
    pub fn new(dir: PathBuf, printer_id: String, printer_name: String) -> Self {
        Self {
            dir,
            printer_id,
            printer_name,
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
            b if b >= 0x20 && b < 0x7F => {
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
        let stem = format!(
            "{}-{}",
            now.format("%Y%m%dT%H%M%S%.9f"),
            self.safe_name()
        );

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
}
