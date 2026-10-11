// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Bytes in, text out, and back — without making the file worse.
//!
//! **Saving an untouched document returns the bytes that were read**, whatever they were: a
//! byte-order mark, CRLF line endings, a missing final newline. That is the template's version
//! of the rule every app built on it should keep (`doc/architecture.md`, rule 7): a program that
//! rewrites a file's line endings because somebody opened it and pressed Save has made the file
//! worse, and the person finds out from a diff that touches every line.
//!
//! So the editing model sees one thing — LF-separated text — and this module remembers what the
//! file actually said and puts it back. Anything it cannot represent faithfully is either kept
//! as text (a lone `\r` in a file that mixes endings) or refused ([`Error::NotUtf8`]).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// How lines end in the file on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    /// `\n` — and also what a file that *mixes* endings is read as, with its `\r`s kept as
    /// characters, so that saving it changes nothing.
    #[default]
    Lf,
    /// `\r\n`, and only when *every* line break in the file is one.
    Crlf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

/// What the file looked like, beyond its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encoding {
    pub line_ending: LineEnding,
    /// Whether the file began with a UTF-8 byte-order mark.
    pub bom: bool,
}

/// Decode a file's bytes into the editing model's text and the [`Encoding`] to save it back in.
pub(crate) fn decode(name: &str, bytes: &[u8]) -> Result<(String, Encoding)> {
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let text = std::str::from_utf8(body).map_err(|error| Error::NotUtf8 {
        name: name.to_string(),
        offset: error.valid_up_to() + if bom { BOM.len() } else { 0 },
    })?;

    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    let line_ending = if crlf > 0 && crlf == lf {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };
    let text = match line_ending {
        LineEnding::Crlf => text.replace("\r\n", "\n"),
        LineEnding::Lf => text.to_string(),
    };
    Ok((text, Encoding { line_ending, bom }))
}

/// The bytes to write for `text` in `encoding` — [`decode`]'s exact inverse.
pub(crate) fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if encoding.bom {
        out.extend_from_slice(BOM);
    }
    match encoding.line_ending {
        LineEnding::Lf => out.extend_from_slice(text.as_bytes()),
        LineEnding::Crlf => out.extend_from_slice(text.replace('\n', "\r\n").as_bytes()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(bytes: &[u8]) -> Vec<u8> {
        let (text, encoding) = decode("t", bytes).unwrap();
        encode(&text, encoding)
    }

    #[test]
    fn every_untouched_file_comes_back_byte_for_byte() {
        for bytes in [
            &b""[..],
            b"plain",
            b"one\ntwo\n",
            b"one\r\ntwo\r\n",
            b"one\r\ntwo",
            b"\xEF\xBB\xBFwith a bom\r\n",
            b"mixed\r\nendings\nstay\rmixed",
            b"\r",
            b"\r\r\n",
        ] {
            assert_eq!(
                round_trip(bytes),
                bytes,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn crlf_is_one_line_break_in_the_model() {
        let (text, encoding) = decode("t", b"a\r\nb").unwrap();
        assert_eq!(text, "a\nb");
        assert_eq!(encoding.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn a_file_that_mixes_endings_keeps_its_carriage_returns_as_text() {
        let (text, encoding) = decode("t", b"a\r\nb\nc").unwrap();
        assert_eq!(text, "a\r\nb\nc");
        assert_eq!(encoding.line_ending, LineEnding::Lf);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_refused_not_replaced() {
        let error = decode("latin1.txt", b"caf\xE9").unwrap_err();
        assert!(matches!(error, Error::NotUtf8 { offset: 3, .. }), "{error}");
    }
}
