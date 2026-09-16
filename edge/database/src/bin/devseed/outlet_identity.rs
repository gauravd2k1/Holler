//! The outlet's identity, read from `seed/outlet.toml`.
//!
//! ONBOARDING A RESTAURANT IS WRITING ONE FILE, NEVER EDITING CODE. Every
//! name, address line, registration, invoice prefix and footer this product
//! prints used to be a `const` in `devseed.rs`; they are all here now, read
//! at run time from a file that is gitignored and per-installation.
//!
//! ## Why a hand-written parser and not the `toml` crate
//!
//! `toml` is not in `edge/database/Cargo.lock`, so adding it means a registry
//! fetch. The file's grammar is flat `key = "value"` with `#` comments and no
//! tables, arrays, numbers or dates — about forty lines to parse exactly, and
//! the parser refuses anything it does not understand rather than guessing.
//! A dependency fetch is the larger risk of the two, and this file's format
//! is a decision we control.
//!
//! ## Why every field is validated and named on failure
//!
//! These values print on a GST invoice. A wrong `state_code` is a wrong
//! place-of-supply; a non-ASCII character is three garbage glyphs on thermal
//! paper and correct everywhere anyone looks (see `invoice_footer_text`
//! below); a missing file with a silent default is somebody else's restaurant
//! name on a client's bill. Every rule below fails loudly, names the field,
//! and quotes what it read.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Everything the product needs to know about WHO this restaurant is.
///
/// Field names match the TOML keys one-for-one, deliberately: an error
/// message naming `state_code` must name the line the operator has to edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutletIdentity {
    pub restaurant_name: String,
    pub legal_name: String,
    pub outlet_name: String,
    pub address_line1: String,
    pub address_line2: Option<String>,
    pub city: String,
    pub state_code: String,
    pub state_name: String,
    pub pincode: String,
    pub gstin: String,
    pub fssai: Option<String>,
    pub invoice_prefix: String,
    pub invoice_footer_text: String,
    pub timezone: String,
    pub day_start_time: String,
    pub upi_vpa: Option<String>,
    pub upi_payee_name: Option<String>,
    pub logo_path: Option<String>,
    /// SHA-256 of the exact bytes this was parsed from, lowercase hex.
    ///
    /// Travels into `seed/demo-outlet.json` and into the bootstrap state
    /// file, so a run can be traced back to the file it was configured by.
    /// A run that seeded from a file nobody can identify afterwards cannot be
    /// reproduced.
    pub source_sha256: String,
}

const REQUIRED_KEYS: &[&str] = &[
    "restaurant_name",
    "legal_name",
    "outlet_name",
    "address_line1",
    "city",
    "state_code",
    "state_name",
    "pincode",
    "gstin",
    "invoice_prefix",
    "invoice_footer_text",
    "timezone",
    "day_start_time",
];

const OPTIONAL_KEYS: &[&str] = &[
    "address_line2",
    "fssai",
    "upi_vpa",
    "upi_payee_name",
    "logo_path",
];

/// Resolution order, most explicit first. There is deliberately NO fallback
/// to `seed/outlet.example.toml`: an installation that never wrote its own
/// file must fail, not quietly seed the demo placeholder restaurant.
///
/// * `--outlet-file <path>` on the command line
/// * `HOLLER_OUTLET_FILE` in the environment
/// * `<repo>/seed/outlet.toml`
pub fn resolve_path(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--outlet-file") {
        if let Some(p) = args.get(i + 1) {
            return PathBuf::from(p);
        }
    }
    if let Ok(p) = std::env::var("HOLLER_OUTLET_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    repo_root().join("seed").join("outlet.toml")
}

/// `edge/database` -> repository root.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

impl OutletIdentity {
    /// Reads and validates the file at `path`.
    ///
    /// A missing file is an error with the remedy in it, not a default.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| {
            format!(
                "outlet identity file {path:?} could not be read: {e}\n  \
                 There is no default: copy seed/outlet.example.toml to \
                 seed/outlet.toml and fill it in, or pass --outlet-file \
                 <path>. See seed/README.md \"Onboarding a new restaurant\"."
            )
        })?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|e| format!("outlet identity file {path:?} is not valid UTF-8: {e}"))?;
        let mut identity = Self::parse(&text, path)?;
        identity.source_sha256 = source_sha256_of(&text);
        Ok(identity)
    }

    /// Parses and validates `text`. Split from [`load`] so every rule below
    /// is testable from a string literal without touching the filesystem.
    pub fn parse(text: &str, origin: &Path) -> Result<Self, String> {
        let pairs = parse_flat_toml(text, origin)?;

        let get = |key: &str| -> Result<String, String> {
            match pairs.get(key) {
                Some(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
                Some(_) => Err(format!(
                    "{}: `{key}` is present but empty. Every required field must carry a value.",
                    origin.display()
                )),
                None => Err(format!(
                    "{}: required field `{key}` is missing. See seed/outlet.example.toml.",
                    origin.display()
                )),
            }
        };
        let get_opt = |key: &str| -> Option<String> {
            pairs
                .get(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };

        let identity = OutletIdentity {
            restaurant_name: get("restaurant_name")?,
            legal_name: get("legal_name")?,
            outlet_name: get("outlet_name")?,
            address_line1: get("address_line1")?,
            address_line2: get_opt("address_line2"),
            city: get("city")?,
            state_code: get("state_code")?,
            state_name: get("state_name")?,
            pincode: get("pincode")?,
            gstin: get("gstin")?,
            fssai: get_opt("fssai"),
            invoice_prefix: get("invoice_prefix")?,
            invoice_footer_text: get("invoice_footer_text")?,
            timezone: get("timezone")?,
            day_start_time: get("day_start_time")?,
            upi_vpa: get_opt("upi_vpa"),
            upi_payee_name: get_opt("upi_payee_name"),
            logo_path: get_opt("logo_path"),
            source_sha256: String::new(),
        };
        identity.validate(origin)?;
        Ok(identity)
    }

    fn validate(&self, origin: &Path) -> Result<(), String> {
        let at = |msg: String| format!("{}: {msg}", origin.display());

        // GSTIN: 15 characters, 2-digit state code + 10-char PAN + entity
        // digit + 'Z' + checksum char. Format only — there is no checksum
        // verification here, because a fixture GSTIN registered to nobody
        // must still pass, and a real one is validated by the tax portal, not
        // by us.
        if !is_gstin_shaped(&self.gstin) {
            return Err(at(format!(
                "`gstin` = \"{}\" is not GSTIN-shaped. Expected 15 characters: \
                 2 digits (state code), 5 letters, 4 digits, 1 letter, \
                 1 alphanumeric (entity), the letter Z, 1 alphanumeric \
                 (checksum) — e.g. 27AAAAA0000A1Z5.",
                self.gstin
            )));
        }

        // THE RULE THAT PAYS FOR THIS WHOLE FILE. state_code is the GST state
        // code and must be the first two digits of the GSTIN, or every
        // invoice this outlet issues carries a wrong place-of-supply — a
        // compliance defect no screen shows and no test catches, because both
        // values are individually well-formed.
        if self.gstin[..2] != self.state_code {
            return Err(at(format!(
                "`state_code` = \"{}\" does not match the first two digits of \
                 `gstin` = \"{}\" (\"{}\"). The GST state code and the GSTIN's \
                 leading digits are the same number; a mismatch puts a wrong \
                 place-of-supply on every invoice.",
                self.state_code,
                self.gstin,
                &self.gstin[..2]
            )));
        }

        if self.pincode.len() != 6 || !self.pincode.bytes().all(|b| b.is_ascii_digit()) {
            return Err(at(format!(
                "`pincode` = \"{}\" is not six digits.",
                self.pincode
            )));
        }

        // invoice_prefix: [A-Z]{1,4} then "/". Lowercase and digits are
        // rejected rather than up-cased: an invoice number is read back to a
        // tax officer, and silently rewriting what the operator typed means
        // the file and the paper disagree.
        if !is_invoice_prefix_shaped(&self.invoice_prefix) {
            return Err(at(format!(
                "`invoice_prefix` = \"{}\" must be one to four CAPITAL letters \
                 followed by \"/\" — e.g. \"SY/\".",
                self.invoice_prefix
            )));
        }

        // ASCII-ONLY, EVERY PRINTED STRING. The ESC/POS stream is emitted as
        // UTF-8 with no codepage translation, so a single non-ASCII byte is
        // garbage on paper and correct in the HTML, the PDF and the .txt
        // companion — every place a developer would look. Applied to the
        // whole printed set, not only the footer, because the footer was
        // simply the first one anybody printed.
        for (field, value) in self.printed_strings() {
            if let Some((index, ch)) = value.char_indices().find(|(_, c)| !c.is_ascii()) {
                return Err(at(format!(
                    "`{field}` contains the non-ASCII character {ch:?} at byte {index}. \
                     Everything printed on a bill reaches the raw ESC/POS byte stream, \
                     which has no codepage translation: a non-ASCII character renders as \
                     garbage on thermal paper while looking correct in the HTML, the PDF \
                     and the .txt companion. Use a plain hyphen for a dash, and ASCII \
                     quotes."
                )));
            }
        }

        // A VPA is `handle@psp`: non-empty either side, exactly one '@', no
        // whitespace. Shape only — whether the address resolves is a question
        // for the payment app, not for a seeder.
        if let Some(vpa) = &self.upi_vpa {
            if !is_vpa_shaped(vpa) {
                return Err(at(format!(
                    "`upi_vpa` = \"{vpa}\" is not a UPI address. Expected \
                     `handle@psp` with exactly one @, no spaces, and both sides \
                     non-empty — e.g. someone@okicici."
                )));
            }
        }

        // `day_start_time` buckets a trading night that crosses midnight into
        // one business date (contracts 0.5.0). HH:MM, 24-hour.
        if !is_hhmm(&self.day_start_time) {
            return Err(at(format!(
                "`day_start_time` = \"{}\" is not a 24-hour HH:MM time — e.g. \"05:00\".",
                self.day_start_time
            )));
        }

        // The timezone is resolved through chrono-tz at write time for every
        // `business_date`, so a name it does not know is a runtime failure on
        // the first sale rather than here. Catch it here instead.
        if self.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(at(format!(
                "`timezone` = \"{}\" is not an IANA timezone name — e.g. \"Asia/Kolkata\". \
                 Every business_date is computed through this zone at write time, so an \
                 unknown name fails on the first sale rather than at seed time.",
                self.timezone
            )));
        }

        // A logo the renderer cannot open is a silently missing mark on every
        // bill. Fail at seed time, where somebody is watching.
        if let Some(rel) = &self.logo_path {
            let resolved = self.resolved_logo_path();
            if !resolved.as_ref().map(|p| p.is_file()).unwrap_or(false) {
                return Err(at(format!(
                    "`logo_path` = \"{rel}\" does not name a readable file (looked at {:?}). \
                     Omit the key entirely for no restaurant mark; a path that does not \
                     resolve would print a bill with a silently missing logo.",
                    resolved.unwrap_or_else(|| PathBuf::from(rel))
                )));
            }
        }

        Ok(())
    }

    /// Every string that reaches printed output, paired with the TOML key an
    /// error should name. Enumerated as a list rather than checked at each
    /// use site: the question "have I found all of them" is answered over a
    /// closed set here, and a field added to the struct without being added
    /// here is the only way to escape the ASCII rule.
    fn printed_strings(&self) -> Vec<(&'static str, &str)> {
        let mut out: Vec<(&'static str, &str)> = vec![
            ("restaurant_name", &self.restaurant_name),
            ("legal_name", &self.legal_name),
            ("outlet_name", &self.outlet_name),
            ("address_line1", &self.address_line1),
            ("city", &self.city),
            ("state_name", &self.state_name),
            ("pincode", &self.pincode),
            ("gstin", &self.gstin),
            ("invoice_prefix", &self.invoice_prefix),
            ("invoice_footer_text", &self.invoice_footer_text),
        ];
        if let Some(v) = &self.address_line2 {
            out.push(("address_line2", v));
        }
        if let Some(v) = &self.fssai {
            out.push(("fssai", v));
        }
        if let Some(v) = &self.upi_payee_name {
            out.push(("upi_payee_name", v));
        }
        out
    }

    /// `logo_path` resolved against the repository root when relative.
    pub fn resolved_logo_path(&self) -> Option<PathBuf> {
        let rel = self.logo_path.as_ref()?;
        let p = PathBuf::from(rel);
        Some(if p.is_absolute() {
            p
        } else {
            repo_root().join(p)
        })
    }
}

/// Parses the flat `key = "value"` grammar and nothing else.
///
/// Rejects, rather than ignores: a table header, an unquoted value, a
/// duplicate key, a key this file does not define. A typo'd key silently
/// dropped is the field that then reads as "missing", and the operator is
/// sent looking at the wrong line.
fn parse_flat_toml(text: &str, origin: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let n = lineno + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            return Err(format!(
                "{}:{n}: table headers are not supported — this file is flat \
                 `key = \"value\"` pairs only. Read: {line}",
                origin.display()
            ));
        }
        let Some((key, rest)) = line.split_once('=') else {
            return Err(format!(
                "{}:{n}: not a `key = \"value\"` pair. Read: {line}",
                origin.display()
            ));
        };
        let key = key.trim().to_string();
        let value = strip_quotes(rest.trim()).ok_or_else(|| {
            format!(
                "{}:{n}: the value for `{key}` must be a double-quoted string. Read: {}",
                origin.display(),
                rest.trim()
            )
        })?;
        if !REQUIRED_KEYS.contains(&key.as_str()) && !OPTIONAL_KEYS.contains(&key.as_str()) {
            let mut known = String::new();
            for k in REQUIRED_KEYS.iter().chain(OPTIONAL_KEYS.iter()) {
                let _ = write!(known, "{k} ");
            }
            return Err(format!(
                "{}:{n}: unknown key `{key}`. This file defines only: {}",
                origin.display(),
                known.trim()
            ));
        }
        if out.insert(key.clone(), value).is_some() {
            return Err(format!(
                "{}:{n}: `{key}` is set twice. Which one wins would be invisible \
                 on every screen it reaches.",
                origin.display()
            ));
        }
    }
    Ok(out)
}

/// A double-quoted value with an optional trailing `#` comment.
///
/// Escapes are NOT interpreted: every value in this file is ASCII text with
/// no backslashes, and quietly turning `\n` into a newline in a name printed
/// on a bill is worse than refusing it. A backslash therefore passes through
/// literally, and a value containing a `"` is not expressible — which is
/// correct for the fields this file holds.
fn strip_quotes(s: &str) -> Option<String> {
    let rest = s.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = &rest[..end];
    let after = rest[end + 1..].trim();
    if !after.is_empty() && !after.starts_with('#') {
        return None;
    }
    Some(value.to_string())
}

fn is_gstin_shaped(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 15 {
        return false;
    }
    b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2..7].iter().all(|c| c.is_ascii_uppercase())
        && b[7..11].iter().all(|c| c.is_ascii_digit())
        && b[11].is_ascii_uppercase()
        && b[12].is_ascii_alphanumeric()
        && b[13] == b'Z'
        && b[14].is_ascii_alphanumeric()
}

fn is_invoice_prefix_shaped(s: &str) -> bool {
    let Some(letters) = s.strip_suffix('/') else {
        return false;
    };
    !letters.is_empty() && letters.len() <= 4 && letters.bytes().all(|b| b.is_ascii_uppercase())
}

fn is_vpa_shaped(s: &str) -> bool {
    if s.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = s.split('@');
    let (Some(handle), Some(psp), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !handle.is_empty() && !psp.is_empty()
}

fn is_hhmm(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else {
        return false;
    };
    if h.len() != 2 || m.len() != 2 {
        return false;
    }
    match (h.parse::<u32>(), m.parse::<u32>()) {
        (Ok(h), Ok(m)) => h < 24 && m < 60,
        _ => false,
    }
}

/// SHA-256, lowercase hex.
///
/// Hand-rolled for the same reason the parser is: this crate has no hashing
/// dependency and the digest is used to identify a config file in a log line,
/// not to protect anything.
/// The identity hash, over NEWLINE-NORMALISED content.
///
/// **The guard this feeds means "a catalogue emitted from a DIFFERENT
/// RESTAURANT'S identity file". Line endings are not a different
/// restaurant.** Hashing the raw bytes made the hash depend on how git
/// happened to check the file out, so a catalogue emitted on an LF checkout
/// could never match on a CRLF one — and both seeders then refuse with
/// "was emitted from a different outlet identity file than this run was
/// handed", which reads exactly like the real defect it is supposed to
/// catch.
///
/// Measured on 2026-09-17, on one unmodified `seed/outlet.example.toml`:
///
/// ```text
/// LF bytes   -> a29e82d47f67…   (what the committed catalogue carries)
/// CRLF bytes -> b4417a7811eb…   (what the Windows CI runner computed)
/// ```
///
/// Five `crash_durability` tests failed on that difference alone. It was
/// invisible until the tests were pointed at the committed example file,
/// which is what finally made this comparison run on CI at all.
///
/// Normalising `\r\n` to `\n` keeps every distinction the guard exists for —
/// one changed byte anywhere in the content still changes the hash — while
/// dropping the one distinction it must not make.
fn source_sha256_of(text: &str) -> String {
    sha256_hex(text.replace("\r\n", "\n").as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }

    let mut out = String::with_capacity(64);
    for word in h {
        let _ = write!(out, "{word:08x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> PathBuf {
        PathBuf::from("seed/outlet.toml")
    }

    /// The committed example, which is also what CI and the drift check read.
    fn example() -> String {
        fs::read_to_string(repo_root().join("seed").join("outlet.example.toml"))
            .expect("seed/outlet.example.toml must exist")
    }

    #[test]
    fn the_committed_example_parses_and_validates() {
        let identity = OutletIdentity::parse(&example(), &origin()).expect("example must be valid");
        assert_eq!(identity.restaurant_name, "Shinjuku Yakitori");
        assert_eq!(identity.legal_name, "Shinjuku Yakitori Hospitality Pvt Ltd");
        assert_eq!(identity.outlet_name, "Shinjuku Yakitori");
        assert_eq!(identity.invoice_prefix, "SY/");
        assert_eq!(identity.gstin, "27AAAAA0000A1Z5");
        assert_eq!(identity.state_code, "27");
        assert_eq!(identity.timezone, "Asia/Kolkata");
        assert_eq!(identity.day_start_time, "05:00");
        // Commented-out optional keys are ABSENT, not empty strings.
        assert_eq!(identity.address_line2, None);
        assert_eq!(identity.upi_vpa, None);
        assert_eq!(identity.logo_path, None);
        assert_eq!(identity.fssai.as_deref(), Some("11522998000123"));
    }

    /// One red case per validation rule. Each mutates the VALID example by a
    /// single line, so a failure cannot be an artefact of an otherwise broken
    /// fixture, and each asserts the message NAMES THE FIELD — an error that
    /// does not say which line to edit is most of the way to no error at all.
    fn rejected(mutation: &str, replacement: &str) -> String {
        let text = example().replace(mutation, replacement);
        assert!(
            text.contains(replacement),
            "the mutation {mutation:?} did not apply — the example file changed \
             and this test is now asserting nothing"
        );
        OutletIdentity::parse(&text, &origin()).expect_err("this mutation must be rejected")
    }

    #[test]
    fn a_gstin_of_the_wrong_shape_is_rejected_by_name() {
        let err = rejected("gstin = \"27AAAAA0000A1Z5\"", "gstin = \"27AAAAA0000A1Z\"");
        assert!(err.contains("`gstin`"), "{err}");
        assert!(err.contains("15 characters"), "{err}");
    }

    #[test]
    fn a_state_code_that_disagrees_with_the_gstin_is_rejected_by_name() {
        // Both values individually well-formed: 29 is Karnataka's real code
        // and the GSTIN is untouched. Only the cross-field rule catches this,
        // which is the whole reason it exists.
        let err = rejected("state_code = \"27\"", "state_code = \"29\"");
        assert!(err.contains("`state_code`"), "{err}");
        assert!(err.contains("place-of-supply"), "{err}");
    }

    #[test]
    fn a_pincode_that_is_not_six_digits_is_rejected_by_name() {
        let err = rejected("pincode = \"411001\"", "pincode = \"41100\"");
        assert!(err.contains("`pincode`"), "{err}");
        assert!(err.contains("six digits"), "{err}");
    }

    #[test]
    fn an_invoice_prefix_of_the_wrong_shape_is_rejected_by_name() {
        for bad in ["dev/", "TOOLONG/", "SY", "SY1/"] {
            let err = rejected(
                "invoice_prefix = \"SY/\"",
                &format!("invoice_prefix = \"{bad}\""),
            );
            assert!(err.contains("`invoice_prefix`"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_non_ascii_printed_string_is_rejected_by_name() {
        // The exact defect observed on paper on 2026-09-13: an em dash in the
        // footer, correct in the HTML, the PDF and the .txt companion, three
        // garbage glyphs on thermal paper.
        let err = rejected(
            "invoice_footer_text = \"Thank you - please visit again\"",
            "invoice_footer_text = \"Thank you \u{2014} please visit again\"",
        );
        assert!(err.contains("`invoice_footer_text`"), "{err}");
        assert!(err.contains("ESC/POS"), "{err}");
    }

    #[test]
    fn the_ascii_rule_covers_every_printed_field_not_only_the_footer() {
        // The footer was simply the first non-ASCII string anyone printed. A
        // restaurant's own name is the likeliest next one.
        let err = rejected(
            "restaurant_name = \"Shinjuku Yakitori\"",
            "restaurant_name = \"Caf\u{e9} Shinjuku\"",
        );
        assert!(err.contains("`restaurant_name`"), "{err}");
    }

    #[test]
    fn a_malformed_upi_vpa_is_rejected_by_name() {
        for bad in [
            "nobody",
            "a@b@c",
            "@okicici",
            "someone@",
            "some one@okicici",
        ] {
            let text = example().replace(
                "# upi_vpa = \"someone@okicici\"",
                &format!("upi_vpa = \"{bad}\""),
            );
            let err = OutletIdentity::parse(&text, &origin())
                .expect_err(&format!("{bad} must be rejected"));
            assert!(err.contains("`upi_vpa`"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_well_formed_upi_vpa_is_accepted_and_optional() {
        let text = example().replace(
            "# upi_vpa = \"someone@okicici\"",
            "upi_vpa = \"someone@okicici\"",
        );
        let identity = OutletIdentity::parse(&text, &origin()).expect("valid vpa");
        assert_eq!(identity.upi_vpa.as_deref(), Some("someone@okicici"));
    }

    #[test]
    fn a_bad_day_start_time_or_timezone_is_rejected_by_name() {
        let err = rejected("day_start_time = \"05:00\"", "day_start_time = \"5:00\"");
        assert!(err.contains("`day_start_time`"), "{err}");
        let err = rejected("timezone = \"Asia/Kolkata\"", "timezone = \"Asia/Poona\"");
        assert!(err.contains("`timezone`"), "{err}");
    }

    #[test]
    fn a_logo_path_that_does_not_resolve_is_rejected_by_name() {
        let text = example().replace(
            "# logo_path = \"seed/assets/restaurant-logo.png\"",
            "logo_path = \"seed/assets/nothing-here.png\"",
        );
        let err = OutletIdentity::parse(&text, &origin()).expect_err("must be rejected");
        assert!(err.contains("`logo_path`"), "{err}");
    }

    #[test]
    fn a_missing_required_field_names_itself() {
        let text = example().replace("city = \"Pune\"", "");
        let err = OutletIdentity::parse(&text, &origin()).expect_err("must be rejected");
        assert!(err.contains("`city`"), "{err}");
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn a_typo_key_is_a_loud_failure_not_a_silent_drop() {
        // A dropped unknown key reads downstream as the REAL key being
        // missing, and sends the operator to the wrong line. Same reasoning
        // as seedfile.go's DisallowUnknownFields.
        let text = example().replace("city = \"Pune\"", "citty = \"Pune\"");
        let err = OutletIdentity::parse(&text, &origin()).expect_err("must be rejected");
        assert!(err.contains("unknown key `citty`"), "{err}");
    }

    #[test]
    fn a_duplicate_key_is_rejected() {
        let text = format!("{}\nrestaurant_name = \"Something Else\"\n", example());
        let err = OutletIdentity::parse(&text, &origin()).expect_err("must be rejected");
        assert!(err.contains("set twice"), "{err}");
    }

    #[test]
    fn an_unquoted_value_is_rejected() {
        let text = example().replace("city = \"Pune\"", "city = Pune");
        let err = OutletIdentity::parse(&text, &origin()).expect_err("must be rejected");
        assert!(err.contains("double-quoted"), "{err}");
    }

    #[test]
    fn a_missing_file_is_an_error_carrying_its_own_remedy() {
        let err = OutletIdentity::load(Path::new("no/such/outlet.toml"))
            .expect_err("a missing file must never be a default");
        assert!(err.contains("seed/outlet.example.toml"), "{err}");
        assert!(err.contains("There is no default"), "{err}");
    }

    #[test]
    fn sha256_matches_known_vectors() {
        // The digest identifies a config file in a log line and in the
        // emitted JSON; a hand-rolled hash that is subtly wrong would still
        // look like a hash. Both vectors are the published SHA-256 values.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one 64-byte block, so the multi-chunk path is covered.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// D26/CRLF: the identity hash must not depend on how git checked the
    /// file out. Falsified in both directions — the same content under two
    /// line-ending conventions hashes the same, and one changed byte does
    /// not.
    #[test]
    fn the_identity_hash_ignores_line_endings_and_nothing_else() {
        let lf = "restaurant_name = \"Shinjuku Yakitori\"\ncity = \"Pune\"\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_ne!(lf, crlf, "the two spellings must really differ as bytes");
        assert_eq!(
            source_sha256_of(lf),
            source_sha256_of(&crlf),
            "LF and CRLF of the SAME file are the same restaurant"
        );

        let changed = lf.replace("Pune", "Mumbai");
        assert_ne!(
            source_sha256_of(lf),
            source_sha256_of(&changed),
            "one changed byte is a different identity and must still be caught"
        );
    }

    #[test]
    fn load_populates_the_source_digest() {
        let path = repo_root().join("seed").join("outlet.example.toml");
        let identity = OutletIdentity::load(&path).expect("example must load");
        assert_eq!(identity.source_sha256.len(), 64);
        assert_eq!(
            identity.source_sha256,
            sha256_hex(&fs::read(&path).unwrap()),
            "the recorded digest must be over the file's exact bytes"
        );
    }
}
