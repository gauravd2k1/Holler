//! `ESCPOS_USB` / `ESCPOS_BLUETOOTH` transport: `printer.address` is a
//! device path — a Windows device path such as `\\.\COM4` for a USB-serial
//! or paired-Bluetooth-SPP printer, or a POSIX character device such as
//! `/dev/usb/lp0` on other platforms — opened for writing. See
//! `transport/mod.rs::build_transport` for why both connection kinds share
//! this one implementation.
//!
//! NOT hardware-verified in this environment (no attached USB/Bluetooth
//! printer). Exercised here only via a fake device path (an ordinary file),
//! which proves the write path but not real device I/O — see this crate's
//! final report for what that does and does not establish.

use std::fs::OpenOptions;
use std::io::Write;

use crate::error::{PrinterError, PrinterResult};
use crate::transport::PrinterTransport;

pub struct PathTransport {
    address: String,
}

impl PathTransport {
    pub fn new(address: String) -> Self {
        Self { address }
    }
}

impl PrinterTransport for PathTransport {
    fn send(&mut self, bytes: &[u8]) -> PrinterResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.address)
            .map_err(|e| PrinterError::Transport {
                printer_id: String::new(),
                address: self.address.clone(),
                message: format!("open: {e}"),
            })?;
        file.write_all(bytes).map_err(|e| PrinterError::Transport {
            printer_id: String::new(),
            address: self.address.clone(),
            message: format!("write: {e}"),
        })?;
        file.flush().map_err(|e| PrinterError::Transport {
            printer_id: String::new(),
            address: self.address.clone(),
            message: format!("flush: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_bytes_to_the_device_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let device_path = dir.path().join("fake_com_port");
        std::fs::write(&device_path, b"").expect("create");

        let mut transport = PathTransport::new(device_path.to_string_lossy().into_owned());
        transport.send(b"ESC/POS bytes").expect("send");

        let written = std::fs::read(&device_path).expect("read back");
        assert_eq!(written, b"ESC/POS bytes");
    }

    #[test]
    fn missing_device_path_is_a_typed_transport_error() {
        let mut transport = PathTransport::new("Z:\\nonexistent\\device\\path".to_string());
        let err = transport.send(b"x").unwrap_err();
        assert!(matches!(err, PrinterError::Transport { .. }));
    }
}
