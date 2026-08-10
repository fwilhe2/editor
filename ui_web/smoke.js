// A UI-free test of the browser boundary.
//
// The sibling of `ffi/csharp-smoke` and `ui_mac`'s `FfiSmoke`: it drives the real
// wasm module against the real `index.html`, so a failure here is a wasm, glue or
// wiring problem, and a page that misbehaves after this passes is CSS.
//
// jsdom has no layout engine — every rectangle it reports is zero — so the shell
// falls back to its metric guards and the viewport is one line tall. That is
// enough to exercise everything except how things look, and it means this runs on
// any machine with node, exactly as the other two smoke tests run without their
// platform's UI toolkit.
//
// Run it through ui_web/smoke.sh, which builds the pieces it needs.

const fs = require("fs");
const path = require("path");
const { JSDOM } = require("jsdom");

const here = __dirname;
const html = fs
  .readFileSync(path.join(here, "index.html"), "utf8")
  // The page loads the ES module build; this harness requires the node one below.
  .replace(/<script type="module">[\s\S]*?<\/script>/, "");

const dom = new JSDOM(html, { pretendToBeVisual: true, url: "http://localhost/" });

// The generated glue type-checks values with `instanceof Window`, `instanceof
// HTMLButtonElement` and so on, so every DOM constructor has to be a real global.
global.window = dom.window;
for (const key of Object.getOwnPropertyNames(dom.window)) {
  if (key in global) continue;
  try {
    global[key] = dom.window[key];
  } catch {
    // Some window properties are getters that throw outside a browser; skip them.
  }
}
global.document = dom.window.document;

// Instantiating the module runs `start()`, which wires the page up.
require(path.join(here, ".smoke/editor_web.js"));

const byId = (id) => document.getElementById(id);
const rendered = () => [...byId("text").children].map((line) => line.textContent).join("\n");
const frame = () => new Promise((resolve) => dom.window.requestAnimationFrame(resolve));

const press = (key, modifiers = {}) =>
  byId("surface").dispatchEvent(
    new dom.window.KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...modifiers })
  );

const wheel = (deltaY) =>
  byId("surface").dispatchEvent(
    new dom.window.WheelEvent("wheel", { deltaY, deltaMode: 0, bubbles: true })
  );

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
  for (const ch of "hello") press(ch);
  await frame();
  check("typing reaches the core", rendered(), "hello");
  check("the status bar counts from 1", byId("position").textContent, "Ln 1, Col 6 · 1 lines · 5 chars");
  check("unsaved changes are marked", byId("name").textContent, "untitled.txt •");
  check("the caret sits on the character grid", byId("caret").style.transform, "translate(40px, 0px)");

  press("ArrowLeft");
  press("ArrowLeft");
  press("Backspace");
  await frame();
  check("arrows and backspace edit at the caret", rendered(), "helo");

  press("z", { ctrlKey: true });
  await frame();
  check("Ctrl+Z undoes", rendered(), "hello");
  press("z", { ctrlKey: true, shiftKey: true });
  await frame();
  check("Ctrl+Shift+Z redoes", rendered(), "helo");
  press("z", { metaKey: true });
  await frame();
  check("⌘Z is the same shortcut", rendered(), "hello");
  check("the undo button follows the history", byId("redo").disabled, false);

  press("Escape");
  const before = rendered();
  press("t", { ctrlKey: true });
  await frame();
  check("Escape and browser shortcuts are not typed", rendered(), before);

  press("End"); // no such capability in the core: must be ignored, not inserted
  press("Enter");
  press("w");
  await frame();
  // The caret is still mid-word from the edits above, so Enter splits "hello" into
  // "hel" and "lo". The viewport is one line tall here, which makes this a test of
  // follow_cursor too: the view has to land on the new line, not stay on the first.
  check("Enter splits the line at the caret and the view follows", rendered(), "wlo");
  check("the status bar sees both lines", byId("position").textContent, "Ln 2, Col 2 · 2 lines · 7 chars");

  wheel(-100);
  await frame();
  check("the wheel scrolls the core's offset back up", rendered(), "hel");
  check("scrolling does not move the caret", byId("position").textContent, "Ln 2, Col 2 · 2 lines · 7 chars");
  check("a caret outside the view is not drawn", byId("caret").style.display, "none");

  // A resize repaints without touching the document — and must not trip over the
  // status message it leaves in place.
  const message = byId("message").textContent;
  dom.window.dispatchEvent(new dom.window.Event("resize"));
  await frame();
  check("a resize repaints and keeps the message", byId("message").textContent, message);
  check("a resize leaves the document alone", rendered(), "hel");

  console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
  process.exit(failures === 0 ? 0 : 1);
})();
