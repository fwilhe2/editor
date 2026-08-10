#!/usr/bin/env python3
"""Drive edit-tui non-interactively, for checking a change without a human at a terminal.

    ui_tui/drive.py FILE                    # start, then quit
    ui_tui/drive.py FILE 'hi\\r' '\\x13' '\\x11'   # type "hi", Enter, Ctrl+S, Ctrl+Q

Each argument after FILE is one burst of input, sent a beat apart so raw mode is
in place before the first one arrives.

This exists because piping into `script` stopped working: ratatui asks the
terminal where the cursor is (ESC[6n) while starting up and blocks until it
answers, and a pipe never does. So this opens a real pty and plays terminal —
including that reply — which is also what makes the exercise honest.

Exits with the editor's own status. Prints what came back with the escape
sequences stripped: enough to see the text, not a screen renderer.
"""

import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time

BINARY = os.environ.get("EDIT_TUI", "target/debug/edit-tui")
CURSOR_QUERY = b"\x1b[6n"
CURSOR_REPORT = b"\x1b[1;1R"  # "the cursor is at row 1, column 1"
ANSI = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]|\x1b[()][A-Z0-9]|\x1b[=>]|\r")
TIMEOUT = 20.0


def main(path, keys):
    keys = [k.encode().decode("unicode_escape").encode("latin-1") for k in keys] or [b"\x11"]

    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.execvp(BINARY, [BINARY, path])

    # A terminal with no size makes ratatui draw nothing at all.
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))

    output = b""
    send_at = time.monotonic() + 1.0
    deadline = time.monotonic() + TIMEOUT

    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:  # the child closed the pty: it has exited
                break
            if not chunk:
                break
            output += chunk
            if CURSOR_QUERY in chunk:
                os.write(fd, CURSOR_REPORT)
        if keys and time.monotonic() >= send_at:
            os.write(fd, keys.pop(0))
            send_at = time.monotonic() + 0.4

    os.close(fd)
    _, status = os.waitpid(pid, 0)
    code = os.waitstatus_to_exitcode(status)

    print(ANSI.sub(b"", output).decode("utf-8", "replace").strip())
    print(f"\n[edit-tui exited {code}]", file=sys.stderr)
    return code


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2:]))
