#!/usr/bin/env node
// Browser UI proof over the mocked-IPC proof build (docs/WINDOWS_PROOF.md §5).
// Starts `vite --mode proof` (or uses --url), drives Chrome/Edge/Chromium over
// CDP through scripts/proof-scenarios.json, prints PASS/FAIL per scenario and
// saves screenshots. Node built-ins only: fetch, WebSocket, child_process.
//
//   pnpm proof:check                       headless, all scenarios
//   pnpm proof:check -- --headed           visible window, pauses on each screenshot
//   pnpm proof:check -- --only tray        scenarios whose name contains "tray"
//   pnpm proof:check -- --url <base>       an already-served proof build (dev or dist-proof)
//   pnpm proof:check -- --cdp <ws-url>     attach to a running browser instead of launching one
//   pnpm proof:check -- --browser <path>   browser binary (or env PROOF_BROWSER)
//   pnpm proof:check -- --out <dir>        screenshot folder (default .proof/screens)
//
// Scenario steps (scripts/proof-scenarios.json), run in order:
//   expectText / expectNoText   page text includes / excludes each string
//   expectSelector              a DOM selector matches
//   expectTheme                 documentElement dataset.theme (waits for the value)
//   expectStyle                 computed style of the first match: { selector,
//                               property, value } (waits for the value)
//   expectField                 a native <select> by label: value, options, enabled
//   expectCall                  a backend command was invoked (optional args match)
//   expectNoCall                a backend command was not invoked (waits 300 ms first)
//   click                       mouse click on the first match (label, optional near)
//   key                         key press (CDP Input.dispatchKeyEvent) at the focused
//                               element: Escape, ArrowLeft, ArrowRight, Home, End
//   select                      set a native <select> by label
//   type                        replace a text field's value (selector + aria-label or
//                               placeholder, text); CDP Input.insertText
//   expectOrder                 textContent of the elements matched by selector equals
//                               texts exactly, in document order
//   screenshot                  PNG into --out (clip "panel" crops to the tray height)
//   wait                        sleep N ms
//
// Exit codes: 0 all scenarios passed, 1 a scenario failed, 2 setup failed.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const STEP_TIMEOUT_MS = 8_000;
const CDP_TIMEOUT_MS = 30_000;

const READY_BY_WINDOW = {
  main: ".tray-panel-reveal--ready",
  flyout: ".tray-panel-reveal--ready",
  settings: ".settings-surface",
  floatbar: ".floatbar__pill, .floatbar__empty",
};

function parseArgs(argv) {
  const opts = { headed: false, only: null, url: null, cdp: null, browser: null, out: null, hold: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) fail(2, `${arg} needs a value`);
      return next;
    };
    if (arg === "--") continue;
    else if (arg === "--headed") opts.headed = true;
    else if (arg === "--only") opts.only = value();
    else if (arg === "--url") opts.url = value().replace(/\/$/, "");
    else if (arg === "--cdp") opts.cdp = value();
    else if (arg === "--browser") opts.browser = value();
    else if (arg === "--out") opts.out = value();
    else if (arg === "--hold") opts.hold = Number(value());
    else fail(2, `unknown option ${arg}`);
  }
  opts.hold ??= opts.headed ? 1_500 : 0;
  opts.out = resolve(opts.out ?? join(appDir, ".proof", "screens"));
  return opts;
}

// Mirrors rust/src/core/redactor.rs SecretRedactor. Output can carry a
// caller-supplied CDP URL (remote bridges often take ?token=) or page text.
const SECRET_PATTERNS = [
  [/(Bearer\s+)[^\s,;]+/gi, "$1[REDACTED]"],
  [/((?:cookie|set-cookie)\s*:\s*)[^\r\n]+/gi, "$1[REDACTED]"],
  [
    /([?&](?:token|code|device_code|verification_code|authorization_code|client_secret|api_key|access_token|refresh_token)=)[^&#\s]+/gi,
    "$1[REDACTED]",
  ],
  [
    /("?(?:api_key|apiKey|token|access_token|refresh_token|id_token|oauth_token|device_code|verification_code|authorization_code|client_secret)"?\s*[:=]\s*")[^"]+"/gi,
    '$1[REDACTED]"',
  ],
  [
    /(\b(?:api_key|access_token|refresh_token|id_token|oauth_token|device_code|verification_code|authorization_code|client_secret|secret|password)\b\s*[:=]\s*)[^\s,;}\]]+/gi,
    "$1[REDACTED]",
  ],
  [/\b(?:sk|ghp|gho|github_pat|zai|nanogpt|openrouter|fk)-[A-Za-z0-9_-]{8,}\b/gi, "[REDACTED]"],
  [/(\/\/)[^/@\s]+@/g, "$1[REDACTED]@"],
];

function redact(text) {
  return SECRET_PATTERNS.reduce((out, [pattern, replacement]) => out.replace(pattern, replacement), String(text));
}

function fail(code, message) {
  console.error(`[proof-check] ${redact(message)}`);
  process.exit(code);
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function findBrowser(explicit) {
  const candidates = [
    explicit,
    process.env.PROOF_BROWSER,
    ...(process.platform === "win32"
      ? [
          `${process.env.ProgramFiles}\\Google\\Chrome\\Application\\chrome.exe`,
          `${process.env["ProgramFiles(x86)"]}\\Google\\Chrome\\Application\\chrome.exe`,
          `${process.env.LOCALAPPDATA}\\Google\\Chrome\\Application\\chrome.exe`,
          `${process.env["ProgramFiles(x86)"]}\\Microsoft\\Edge\\Application\\msedge.exe`,
          `${process.env.ProgramFiles}\\Microsoft\\Edge\\Application\\msedge.exe`,
        ]
      : process.platform === "darwin"
        ? [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
          ]
        : ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/microsoft-edge"]),
  ];
  return candidates.find((path) => path && existsSync(path)) ?? null;
}

async function waitForHttp(url, timeoutMs) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      if ((await fetch(url)).ok) return true;
    } catch {
      // not up yet
    }
    await sleep(250);
  }
  return false;
}

async function startProofServer() {
  const vite = join(appDir, "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(vite)) fail(2, "vite is not installed; run pnpm install");
  // A fixed port let a parallel run in another worktree answer this run's
  // requests, so each run takes its own free port unless one is pinned.
  const port = process.env.PROOF_PORT ? Number(process.env.PROOF_PORT) : await freePort();
  if (!Number.isInteger(port) || port <= 0 || port > 65_535) fail(2, `invalid PROOF_PORT ${process.env.PROOF_PORT}`);
  const child = spawn(
    process.execPath,
    [vite, "--mode", "proof", "--port", String(port), "--strictPort", "--host", "127.0.0.1"],
    { cwd: appDir, stdio: ["ignore", "pipe", "pipe"] },
  );
  child.once("error", (error) => fail(2, `proof server failed to start: ${error.message}`));
  // fail() exits the process directly; do not leave the server behind.
  process.once("exit", () => child.kill());
  let exited = false;
  child.once("exit", () => (exited = true));
  let log = "";
  child.stdout.on("data", (chunk) => (log += chunk));
  child.stderr.on("data", (chunk) => (log += chunk));
  const base = `http://127.0.0.1:${port}`;
  // With --strictPort a taken port makes vite exit; stop polling then, or the
  // other process on that port would be tested instead.
  const end = Date.now() + 30_000;
  let up = false;
  while (!up && !exited && Date.now() < end) up = await waitForHttp(`${base}/proof.html`, 250);
  if (!up || exited) {
    child.kill();
    fail(2, `proof server did not start on port ${port}:\n${log}`);
  }
  return { base, stop: () => child.kill() };
}

async function launchBrowser(opts) {
  const binary = findBrowser(opts.browser);
  if (!binary) fail(2, "no Chrome, Edge or Chromium found; pass --browser or set PROOF_BROWSER");
  // Reused across runs: an isolated profile with no logins, never the user's.
  const profile = join(appDir, ".proof", "browser-profile");
  mkdirSync(profile, { recursive: true });
  // A fixed free port rather than port 0 + DevToolsActivePort: that file
  // outlives the browser, so a stale copy could point at an older instance.
  const port = await freePort();
  const args = [
    `--user-data-dir=${profile}`,
    `--remote-debugging-port=${port}`,
    "--no-first-run",
    "--no-default-browser-check",
    // A headed window behind other windows would otherwise stop painting,
    // and the tray panel waits on requestAnimationFrame before it reveals.
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--disable-background-timer-throttling",
    "--window-size=1000,1080",
    ...(opts.headed ? [] : ["--headless=new", "--disable-gpu"]),
    "about:blank",
  ];
  // A binary that cannot run makes spawn() throw (Windows: EFTYPE) or emit
  // "error" rather than "exit"; both must reach the setup-error path.
  let child;
  try {
    child = spawn(binary, args, { stdio: "ignore" });
  } catch (error) {
    fail(2, `could not launch browser ${binary}: ${error.message}`);
  }
  let exited = false;
  child.once("exit", () => (exited = true));
  let launchError = null;
  child.once("error", (error) => (launchError = error));
  const end = Date.now() + 20_000;
  while (Date.now() < end) {
    await sleep(200);
    if (launchError) fail(2, `could not launch browser ${binary}: ${launchError.message}`);
    try {
      const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      if (version.webSocketDebuggerUrl) return { ws: version.webSocketDebuggerUrl, child };
    } catch {
      // not listening yet
    }
    if (exited) {
      fail(
        2,
        `browser exited before opening its debugging port. Another browser may still be using ${profile}; close it and retry.`,
      );
    }
  }
  child.kill();
  fail(2, `browser did not open a debugging port: ${binary}`);
}

function freePort() {
  return new Promise((done, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 0;
    this.pending = new Map();
    this.listeners = [];
    socket.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id !== undefined && this.pending.has(msg.id)) {
        const { resolve: done, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) reject(new Error(`${msg.error.message} (${msg.error.code})`));
        else done(msg.result);
      } else if (msg.method) {
        for (const listener of this.listeners) listener(msg);
      }
    });
    socket.addEventListener("close", () => {
      for (const { reject } of this.pending.values()) reject(new Error("browser connection closed"));
      this.pending.clear();
    });
  }

  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((done, reject) => {
      socket.addEventListener("open", done, { once: true });
      socket.addEventListener("error", () => reject(new Error(`cannot connect to ${url}`)), { once: true });
    });
    return new Cdp(socket);
  }

  send(method, params = {}, sessionId) {
    // A closed socket drops sends silently; fail now rather than at the timeout.
    if (this.socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error(`CDP ${method}: browser connection closed`));
    }
    const id = ++this.nextId;
    this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    return new Promise((done, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP ${method} timed out after ${CDP_TIMEOUT_MS} ms`));
      }, CDP_TIMEOUT_MS);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          done(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
    });
  }

  on(listener) {
    this.listeners.push(listener);
  }

  close() {
    this.socket.close();
  }
}

class Page {
  constructor(cdp, sessionId) {
    this.cdp = cdp;
    this.sessionId = sessionId;
    this.problems = [];
    cdp.on((msg) => {
      if (msg.sessionId !== sessionId) return;
      if (msg.method === "Runtime.consoleAPICalled" && ["error", "warning"].includes(msg.params.type)) {
        const text = msg.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" ");
        this.problems.push(`console.${msg.params.type}: ${text.slice(0, 300)}`);
      } else if (msg.method === "Runtime.exceptionThrown") {
        const detail = msg.params.exceptionDetails;
        this.problems.push(`exception: ${(detail.exception?.description ?? detail.text).slice(0, 300)}`);
      }
    });
  }

  send(method, params) {
    return this.cdp.send(method, params, this.sessionId);
  }

  async eval(expression) {
    const result = await this.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? "evaluate failed");
    return result.result.value;
  }

  async poll(expression, timeoutMs = STEP_TIMEOUT_MS) {
    const end = Date.now() + timeoutMs;
    for (;;) {
      if (await this.eval(expression)) return true;
      if (Date.now() > end) return false;
      await sleep(150);
    }
  }
}

const js = (value) => JSON.stringify(value);
// Windows virtual-key codes for the keys a `key` step may press.
const KEY_CODES = { Escape: 27, End: 35, Home: 36, ArrowLeft: 37, ArrowRight: 39 };
const asList = (value) => (Array.isArray(value) ? value : [value]);

// textContent rather than innerText: the app splits emails across elements,
// which innerText reads back with layout spaces in between.
const hasText = (text) => `document.body.textContent.includes(${js(text)})`;

// A native <select> named by its Field label (Field wires aria-labelledby to the label span).
const selectByLabel = (label) => `[...document.querySelectorAll("select")].find((el) => {
  const id = el.getAttribute("aria-labelledby");
  return id !== null && document.getElementById(id)?.textContent.trim() === ${js(label)};
}) ?? null`;

async function runStep(page, step, ctx) {
  if (step.expectText !== undefined) {
    for (const text of asList(step.expectText)) {
      if (!(await page.poll(hasText(text)))) throw new Error(`text not found: ${js(text)}`);
    }
  } else if (step.expectNoText !== undefined) {
    for (const text of asList(step.expectNoText)) {
      if (!(await page.poll(`!${hasText(text)}`))) throw new Error(`text still present: ${js(text)}`);
    }
  } else if (step.expectSelector !== undefined) {
    if (!(await page.poll(`!!document.querySelector(${js(step.expectSelector)})`))) {
      throw new Error(`selector not found: ${step.expectSelector}`);
    }
  } else if (step.expectNoOverflow !== undefined) {
    // Each match must fit its box horizontally: wrapped text never overflows,
    // while text cut by `white-space: nowrap` + `text-overflow` does.
    const selector = js(step.expectNoOverflow);
    if (!(await page.poll(`!!document.querySelector(${selector})`))) {
      throw new Error(`selector not found: ${step.expectNoOverflow}`);
    }
    const clipped = await page.eval(`[...document.querySelectorAll(${selector})]
      .filter((el) => el.scrollWidth > el.clientWidth + 1)
      .map((el) => el.textContent)`);
    if (clipped.length > 0) throw new Error(`text clipped in ${step.expectNoOverflow}: ${js(clipped)}`);
  } else if (step.expectTheme !== undefined) {
    // Polled: under `auto` the theme arrives after an async shell read.
    const theme = "document.documentElement.dataset.theme";
    if (!(await page.poll(`${theme} === ${js(step.expectTheme)}`))) {
      throw new Error(`theme is ${js(await page.eval(theme))}, expected ${js(step.expectTheme)}`);
    }
  } else if (step.expectStyle !== undefined) {
    const { selector, property, value } = step.expectStyle;
    const read = `(() => {
      const el = document.querySelector(${js(selector)});
      return el ? getComputedStyle(el).getPropertyValue(${js(property)}).trim() : null;
    })()`;
    if (!(await page.poll(`${read} === ${js(value)}`))) {
      const actual = await page.eval(read);
      if (actual === null) throw new Error(`selector not found: ${selector}`);
      throw new Error(`${selector} ${property} is ${js(actual)}, expected ${js(value)}`);
    }
  } else if (step.click !== undefined) {
    // `near` picks, among same-label matches, the one whose closest ancestor
    // containing that text is nearest (e.g. the Switch button of one account row).
    const { selector, label, near = null } = step.click;
    const locate = `(() => {
      const near = ${js(near)};
      const depth = (e) => {
        let d = 0;
        for (let n = e; n; n = n.parentElement, d++) if (n.textContent.includes(near)) return d;
        return Infinity;
      };
      const matches = [...document.querySelectorAll(${js(selector)})].filter(
        (e) => (e.getAttribute("aria-label") || e.textContent || "").trim() === ${js(label)});
      const el = near === null
        ? matches[0]
        : matches.filter((e) => depth(e) < Infinity).sort((a, b) => depth(a) - depth(b))[0];
      if (!el) return null;
      el.scrollIntoView({ block: "center" });
      const r = el.getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    })()`;
    // Targets can render after async fixture data (e.g. a count in a summary
    // label), so wait for them like the expect* steps do.
    const end = Date.now() + STEP_TIMEOUT_MS;
    let point = await page.eval(locate);
    while (!point && Date.now() <= end) {
      await sleep(150);
      point = await page.eval(locate);
    }
    if (!point) throw new Error(`nothing to click: ${selector} labelled ${js(label)}`);
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
      await page.send("Input.dispatchMouseEvent", { type, ...point, button: "left", clickCount: 1 });
    }
  } else if (step.expectOrder !== undefined) {
    // Exact textContent list of the matched elements, in document order.
    const { selector, texts } = step.expectOrder;
    const listed = `[...document.querySelectorAll(${js(selector)})].map((e) => e.textContent.trim())`;
    const ordered = await page.poll(`JSON.stringify(${listed}) === ${js(JSON.stringify(texts))}`);
    if (!ordered) throw new Error(`order mismatch in ${selector}: ${js(await page.eval(listed))}, expected ${js(texts)}`);
  } else if (step.expectCall !== undefined) {
    const { cmd, args = {} } = step.expectCall;
    const found = await page.poll(`(window.__CODEXBAR_PROOF__?.calls() ?? []).some((call) =>
      call.cmd === ${js(cmd)} && Object.entries(${js(args)}).every(([k, v]) => JSON.stringify(call.args[k]) === JSON.stringify(v)))`);
    if (!found) throw new Error(`backend call not made: ${cmd} ${js(args)}`);
  } else if (step.screenshot !== undefined) {
    await sleep(300);
    let clip;
    if (step.clip === "panel") {
      // The browser window cannot resize, so crop to the height the tray
      // panel committed for its window.
      const height = await page.eval(
        `parseFloat(document.querySelector(".menu-surface--tray")?.style.maxHeight || "0")`,
      );
      if (height > 0) clip = { x: 0, y: 0, width: ctx.width, height: Math.ceil(height), scale: 1 };
    }
    const shot = await page.send("Page.captureScreenshot", { format: "png", ...(clip ? { clip } : {}) });
    const file = join(ctx.out, `${step.screenshot}.png`);
    writeFileSync(file, Buffer.from(shot.data, "base64"));
    ctx.screenshots.push(file);
    if (ctx.hold) await sleep(ctx.hold);
  } else if (step.select !== undefined) {
    // Native <select> ignores mouse clicks here: set the value through the
    // prototype setter and fire `change`, which is what React's onChange reads.
    const { label, value } = step.select;
    const ok = await page.eval(`(() => {
      const el = ${selectByLabel(label)};
      if (!el || ![...el.options].some((o) => o.value === ${js(value)})) return false;
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, ${js(value)});
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    })()`);
    if (!ok) throw new Error(`cannot select ${js(value)} in ${js(label)}`);
  } else if (step.expectField !== undefined) {
    // Polls until the control matches, so a re-render after a save can settle.
    const { label, value, options, enabled } = step.expectField;
    const mismatch = `(() => {
      const el = ${selectByLabel(label)};
      if (!el) return "no select labelled ${label}";
      ${value !== undefined ? `if (el.value !== ${js(value)}) return "value is " + JSON.stringify(el.value);` : ""}
      ${options !== undefined ? `const texts = [...el.options].map((o) => o.textContent.trim());
      const want = ${js(options)};
      if (JSON.stringify(texts) !== JSON.stringify(want)) return "options are " + JSON.stringify(texts);` : ""}
      ${enabled !== undefined ? `if (el.disabled === ${js(enabled)}) return "enabled is " + !el.disabled;` : ""}
      return "";
    })()`;
    if (!(await page.poll(`${mismatch} === ""`))) throw new Error(await page.eval(mismatch));
  } else if (step.expectNoCall !== undefined) {
    // A call that would have been made lands during the same event turn; the
    // delay gives an asynchronous one a chance to be recorded before checking.
    await sleep(300);
    const { cmd } = step.expectNoCall;
    const made = await page.eval(
      `(window.__CODEXBAR_PROOF__?.calls() ?? []).some((call) => call.cmd === ${js(cmd)})`,
    );
    if (made) throw new Error(`unexpected backend call: ${cmd}`);
  } else if (step.key !== undefined) {
    // Keys go to the focused element, as a real key press would.
    const info = KEY_CODES[step.key];
    if (!info) throw new Error(`unsupported key ${js(step.key)}`);
    const params = { key: step.key, code: step.key, windowsVirtualKeyCode: info, nativeVirtualKeyCode: info };
    await page.send("Input.dispatchKeyEvent", { type: "keyDown", ...params });
    await page.send("Input.dispatchKeyEvent", { type: "keyUp", ...params });
  } else if (step.type !== undefined) {
    // Focus a text field (matched by aria-label or placeholder within `selector`),
    // replace its value, and insert via CDP so React sees a real input event.
    const { selector, label, text } = step.type;
    const focused = await page.poll(`(() => {
      const el = [...document.querySelectorAll(${js(selector)})].find((e) =>
        (e.getAttribute("aria-label") || e.getAttribute("placeholder") || "").trim() === ${js(label)});
      if (!el) return false;
      el.scrollIntoView({ block: "center" });
      el.focus();
      el.select();
      return true;
    })()`);
    if (!focused) throw new Error(`no field to type into: ${selector} labelled ${js(label)}`);
    await page.send("Input.insertText", { text });
  } else if (step.wait !== undefined) {
    await sleep(step.wait);
  } else {
    throw new Error(`unknown step ${js(step)}`);
  }
}

async function runScenario(page, base, scenario, opts) {
  const [width, height] = scenario.size;
  const window = new URLSearchParams(scenario.query).get("window") ?? "main";
  const ready = READY_BY_WINDOW[window];
  if (!ready) throw new Error(`unknown window ${window}`);
  page.problems = [];
  await page.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await page.send("Page.navigate", { url: `${base}/proof.html?${scenario.query}` });
  if (!(await page.poll(`!!document.querySelector(${js(ready)})`, 20_000))) {
    throw new Error(`surface never became ready (${ready})`);
  }
  const ctx = { out: opts.out, hold: opts.hold, width, screenshots: [] };
  for (const step of scenario.steps) await runStep(page, step, ctx);
  const unhandled = await page.eval("window.__CODEXBAR_PROOF__?.unhandledCommands() ?? []");
  if (unhandled.length) throw new Error(`unhandled backend commands: ${unhandled.join(", ")}`);
  const problems = page.problems.filter((line) => !/Download the React DevTools/.test(line));
  if (problems.length) throw new Error(problems.join("\n      "));
  return ctx.screenshots;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const { scenarios } = JSON.parse(readFileSync(join(appDir, "scripts", "proof-scenarios.json"), "utf8"));
  const selected = scenarios.filter((s) => !opts.only || s.name.includes(opts.only));
  if (!selected.length) fail(2, `no scenario matches ${js(opts.only)}`);
  mkdirSync(opts.out, { recursive: true });

  const server = opts.url ? null : await startProofServer();
  const base = opts.url ?? server.base;
  const browser = opts.cdp ? null : await launchBrowser(opts);
  let cdp;
  let targetId;
  let failed = 0;
  try {
    cdp = await Cdp.connect(opts.cdp ?? browser.ws);
    ({ targetId } = await cdp.send("Target.createTarget", { url: "about:blank" }));
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    const page = new Page(cdp, sessionId);
    await page.send("Runtime.enable");
    await page.send("Page.enable");
    if (opts.headed) await page.send("Page.bringToFront");

    for (const scenario of selected) {
      try {
        const shots = await runScenario(page, base, scenario, opts);
        console.log(`PASS  ${scenario.name}${shots.length ? `  (${shots.length} screenshot${shots.length > 1 ? "s" : ""})` : ""}`);
      } catch (error) {
        failed++;
        console.log(`FAIL  ${scenario.name}\n      ${redact(error.message)}`);
      }
    }
  } finally {
    if (cdp && targetId) await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    if (browser) {
      if (cdp) await cdp.send("Browser.close").catch(() => browser.child.kill());
      else browser.child.kill();
    }
    cdp?.close();
    server?.stop();
  }
  console.log(`\n${selected.length - failed}/${selected.length} scenarios passed. Screenshots: ${opts.out}`);
  process.exit(failed ? 1 : 0);
}

main().catch((error) => fail(2, error.stack ?? String(error)));
