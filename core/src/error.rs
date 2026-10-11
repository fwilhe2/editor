// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

/// Everything the core can refuse to do.
///
/// Each variant is a sentence a shell can show as it is — the `Display` text is written for the
/// person using the program, not for whoever wrote it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the document has never been saved; choose where to save it")]
    NoPath,

    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The bytes are not UTF-8. Refused rather than decoded lossily: a lossy decode followed by
    /// a save would replace every undecodable byte in the file with U+FFFD, and saving must
    /// never make an existing file worse (`doc/architecture.md`, rule 7).
    #[error("{name} is not UTF-8 text (first bad byte at offset {offset})")]
    NotUtf8 { name: String, offset: usize },
}

pub type Result<T> = std::result::Result<T, Error>;
