//! Minimal, hand-rolled ESC/POS byte builder. No crate dependency: the
//! command set this crate uses (init, bold, align, feed, cut, plain text) is
//! a handful of well-documented byte sequences common to every ESC/POS
//! thermal printer regardless of brand, and ADR-013 asks that we not pull in
//! a dependency (with a C toolchain requirement or otherwise) where a dozen
//! constant byte slices do the job on 4GB spinning-disk outlet hardware.
//!
//! Vendor/brand differences are NOT modelled here — only the transport
//! (`src/transport`) varies per adapter; the byte stream is the same ESC/POS
//! subset for every printer this crate talks to (docs/spec/hardware-printing.md
//! "Vendor/brand differences are adapter details, not new contract variants").

/// Builds an ESC/POS byte stream incrementally. Every method appends bytes
/// and returns `&mut Self` for chaining.
#[derive(Debug, Default)]
pub struct EscPosBuilder {
    buf: Vec<u8>,
}

impl EscPosBuilder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// ESC @ — reset the printer to its power-on default state. Always the
    /// first bytes of a ticket, so a printer left in a bold/centered state
    /// by a previous job does not bleed into this one.
    pub fn init(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[0x1B, 0x40]);
        self
    }

    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.buf
            .extend_from_slice(&[0x1B, 0x45, if on { 1 } else { 0 }]);
        self
    }

    /// ESC ! n — double-width/double-height text. Used for the station name
    /// and sequence marker, which must be legible at a glance across a
    /// kitchen pass.
    pub fn double_size(&mut self, on: bool) -> &mut Self {
        self.buf
            .extend_from_slice(&[0x1B, 0x21, if on { 0x30 } else { 0x00 }]);
        self
    }

    pub fn align_center(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[0x1B, 0x61, 1]);
        self
    }

    pub fn align_left(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[0x1B, 0x61, 0]);
        self
    }

    /// Raw ASCII text. KOTs are internal kitchen tickets (station names,
    /// menu item names, notes) — UTF-8 beyond ASCII is not guaranteed to
    /// render on a given printer's built-in code page, so callers should
    /// stick to what the till already stores as printable menu text. This
    /// crate does not attempt code-page translation.
    pub fn text(&mut self, s: &str) -> &mut Self {
        self.buf.extend_from_slice(s.as_bytes());
        self
    }

    pub fn line(&mut self, s: &str) -> &mut Self {
        self.text(s);
        self.newline()
    }

    pub fn newline(&mut self) -> &mut Self {
        self.buf.push(b'\n');
        self
    }

    pub fn rule(&mut self, width_chars: usize) -> &mut Self {
        self.line(&"-".repeat(width_chars))
    }

    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.buf.extend_from_slice(&[0x1B, 0x64, lines]);
        self
    }

    /// GS V 1 — partial cut. Ends every ticket so it separates cleanly from
    /// the next.
    pub fn cut(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[0x1D, 0x56, 0x01]);
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_emits_esc_at() {
        let mut b = EscPosBuilder::new();
        b.init();
        let bytes = b.into_bytes();
        assert_eq!(bytes, vec![0x1B, 0x40]);
    }

    #[test]
    fn cut_is_last_bytes_in_a_chain() {
        let mut b = EscPosBuilder::new();
        b.line("hi").cut();
        let bytes = b.into_bytes();
        assert_eq!(&bytes[bytes.len() - 3..], &[0x1D, 0x56, 0x01]);
    }

    #[test]
    fn bold_toggles_on_and_off() {
        let mut b = EscPosBuilder::new();
        b.bold(true).text("x").bold(false);
        let bytes = b.into_bytes();
        assert_eq!(&bytes[0..3], &[0x1B, 0x45, 1]);
        assert_eq!(&bytes[bytes.len() - 3..], &[0x1B, 0x45, 0]);
    }
}
