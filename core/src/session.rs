// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::document::Position;
use crate::history::History;
use crate::view::Scroll;

/// What a process that exits must hand to the next one for editing to carry on: the caret, the
/// selection, the scroll position and the history.
///
/// The CLI is stateless — each invocation loads the file, applies one command and writes it —
/// so without this an `undo` would have nothing to undo. `--session FILE` stores one of these
/// as JSON between invocations. A GUI shell never needs it; its process *is* the session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub caret: Position,
    #[serde(default)]
    pub anchor: Option<Position>,
    #[serde(default)]
    pub scroll: Scroll,
    #[serde(default)]
    pub history: History,
    /// The last query, so `quill find-next` means what Find Next means in a window.
    #[serde(default)]
    pub search: Option<String>,
}
