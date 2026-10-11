// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

// A UI-free test of the browser boundary: the real wasm module against the real index.html.
//
// A failure here is a wasm, glue or wiring problem; a page that misbehaves after this passes is
// CSS. jsdom has no layout engine and no canvas — every rectangle is zero and nothing can be
// measured — so the shell falls back to its guards: no wrapping, one row tall, eight pixels a
// character. That exercises everything except how things look.
//
// Run it through ui_web/smoke.sh, which builds the pieces it needs.

const fs = require("fs");
const path = require("path");
const { JSDOM, VirtualConsole } = require("jsdom");

const here = __dirname;
const html = fs
  .readFileSync(path.join(here, "index.html"), "utf8")
  // The page loads the ES module build; this harness requires the node one below.
  .replace(/<script type="module">[\s\S]*?<\/script>/, "");

// jsdom says "not implemented" for a canvas context, which is the fallback being exercised.
const quiet = new VirtualConsole();
quiet.on("jsdomError", (e) => {
  if (!String(e.message).includes("Not implemented")) console.error(e);
});
const dom = new JSDOM(html, {
  pretendToBeVisual: true,
  url: "http://localhost/",
  virtualConsole: quiet,
});

// The glue type-checks values with `instanceof Window` and friends, so every DOM constructor
// has to be a real global.
global.window = dom.window;
for (const key of Object.getOwnPropertyNames(dom.window)) {
  if (key in global) continue;
  try {
    global[key] = dom.window[key];
  } catch {
    // Some window properties are getters that throw outside a browser.
  }
}
global.document = dom.window.document;

// Instantiating the module runs `start()`, which wires the page up.
require(path.join(here, ".smoke/quill_web.js"));

const byId = (id) => document.getElementById(id);
const rows = () => [...document.querySelectorAll("#rows .row")].map((r) => r.textContent).join("\n");
const frame = () => new Promise((resolve) => dom.window.requestAnimationFrame(resolve));

const type = (text) => {
  for (const c of text) {
    byId("input").value = c;
    byId("input").dispatchEvent(new dom.window.InputEvent("input", { bubbles: true }));
  }
};
const press = (target, key, modifiers = {}) =>
  target.dispatchEvent(
    new dom.window.KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...modifiers })
  );
const key = (k, modifiers) => press(byId("input"), k, modifiers);

let failures = 0;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failures += 1;
  console.log(
    `${ok ? "ok  " : "FAIL"} ${label}` +
      (ok ? "" : `\n       expected ${JSON.stringify(expected)}\n       got      ${JSON.stringify(actual)}`)
  );
};

(async () => {
  type("hello");
  await frame();
  check("typing through the hidden input reaches the core", rows(), "hello");
  check("the input is emptied — it never holds the document", byId("input").value, "");
  check("the status bar counts from 1", byId("position").textContent, "Ln 1, Col 6 · 1 lines · LF");
  check("unsaved changes are marked", byId("name").textContent, "Untitled •");
  check("the caret is placed by the core's measurements", byId("caret").style.transform, "translate(40px, 0px)");

  key("ArrowLeft");
  key("ArrowLeft");
  key("Backspace");
  await frame();
  check("arrows and Backspace edit at the caret", rows(), "helo");

  key("z", { ctrlKey: true });
  await frame();
  check("Ctrl+Z undoes", rows(), "hello");
  key("z", { ctrlKey: true, shiftKey: true });
  await frame();
  check("Ctrl+Shift+Z redoes", rows(), "helo");
  key("z", { metaKey: true });
  await frame();
  check("⌘Z is the same shortcut", rows(), "hello");
  check(
    "a toolbar button is enabled by the history",
    document.querySelector('[data-command="edit.redo"]').hasAttribute("disabled"),
    false
  );

  const before = rows();
  key("t", { ctrlKey: true });
  await frame();
  check("the browser's own shortcuts are left alone", rows(), before);

  key("End");
  key("Enter");
  type("w");
  await frame();
  // One row tall in jsdom, so this is also a test of follow_caret: the view has to land on the
  // new line rather than stay on the first.
  check("Enter splits the line and the view follows the caret", rows(), "w");
  check("the status bar sees both lines", byId("position").textContent, "Ln 2, Col 2 · 2 lines · LF");

  byId("page").dispatchEvent(new dom.window.WheelEvent("wheel", { deltaY: -3, deltaMode: 1, bubbles: true, cancelable: true }));
  await frame();
  check("the wheel scrolls the core's position", rows(), "hello");
  check("scrolling leaves the caret where it was", byId("position").textContent, "Ln 2, Col 2 · 2 lines · LF");
  check("a caret scrolled out of view is not drawn", byId("caret").style.display, "none");

  key("k", { ctrlKey: true });
  check("Ctrl+K opens the palette", byId("palette").hidden, false);
  byId("palette-input").value = "select all";
  byId("palette-input").dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  check("the palette ranks the core's commands", document.querySelector("#palette-list li .label").textContent, "Select All");
  press(byId("palette-input"), "Enter");
  await frame();
  check("and runs the one chosen", document.querySelectorAll("#rows .selection").length, 1);
  check("the palette closes", byId("palette").hidden, true);

  key("f", { ctrlKey: true });
  check("Ctrl+F opens the find bar", byId("findbar").hidden, false);
  byId("find-input").value = "ell";
  press(byId("find-input"), "Enter");
  await frame();
  check("Enter finds and the status follows", byId("position").textContent, "Ln 1, Col 5 · 2 lines · LF");

  dom.window.confirm = () => true;
  document.querySelector('[data-command="file.new"]').click();
  await frame();
  check("New, after confirming, empties the document", rows(), "");
  check("and it is clean", byId("name").textContent, "Untitled");

  console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
  process.exit(failures === 0 ? 0 : 1);
})();
