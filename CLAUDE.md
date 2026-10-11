<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# CLAUDE.md

The instructions for every agent are in `AGENTS.md`, imported here so there is one copy:

@AGENTS.md

## Claude Code specifics

- **Starting a new application from this template** is the `bootstrap` skill
  (`.claude/skills/bootstrap/SKILL.md`), which walks `doc/bootstrap.md`.
- Before reporting a change done, run `scripts/check.sh` and say which gates it skipped on this
  machine; a skipped gate is not a passed one.
- Screenshots of the GUI shells from Linux: `doc/ci.md`, "Looking at the GUIs from Linux". Read the
  image before describing what a shell looks like.
