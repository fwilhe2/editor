---
name: bootstrap
description: Start a new application from this shared-core, native-shell template — rename it, replace the example text editor's domain, and keep every shell, ratchet and CI job working. Use when asked to create, bootstrap, scaffold or start a new app/project from this repository or template.
---

<!--
SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>

SPDX-License-Identifier: MIT
-->

# Bootstrap an application from this template

Follow `doc/bootstrap.md` from step 0, in order. It is the procedure; this skill only frames it.

1. **Ask first** (step 0): the slug and name, the application ID, the repository URL, the licence
   and copyright holder, the domain in a paragraph with its five most important verbs, and which
   shells to keep. These are the person's decisions — ask in one message and wait for the answers.
2. **Rename mechanically** with `scripts/bootstrap.py`, then `scripts/bootstrap.py --check quill`.
3. **Prove the rename** with `scripts/check.sh` before changing anything else, and commit if the
   person has asked you to commit.
4. **Replace the domain core-first**, then the CLI and `examples/sample.sh`, then one shell at a
   time — each step leaving every test green. `doc/architecture.md` is normative throughout; its
   checklists ("Adding a capability", "Adding a shell") apply to every verb you add.
5. **Look at every GUI shell** you changed (`scripts/run.sh`, or the screenshot recipes in
   `doc/ci.md`) and read the screenshots before describing them.
6. **Finish** (step 9): delete the template-only files, rewrite `AGENTS.md`'s first section, and
   report what was verified, how, what was not (the macOS shell cannot run off a Mac), and every
   named gap.

Do not skip the ratchets to get green: a failing parity, packaging or menu test is telling you a
verb, a package or a key is missing somewhere. Fix the cause.
