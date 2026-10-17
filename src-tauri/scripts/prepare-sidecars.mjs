#!/usr/bin/env node
//
// Prepare Tauri sidecars before `tauri build` / `tauri dev` consume them.
//
// What it does:
//   1. Resolves the target triple — `--target <triple>` arg, or
//      `TAURI_TARGET_TRIPLE` env, or the host's `rustc -vV` host triple.
//   2. Runs `cargo build --release --no-default-features`, with the feature
//      each binary target requires, for each sidecar bin (`rc-mcp`,
//      `rc-computer-helper`) for that triple from `src-tauri/`.
//   3. Copies each produced binary to
//      `src-tauri/binaries/<bin>-<triple>{.exe}` so Tauri's externalBin
//      bundler picks it up under its bare name at install time.
//   4. Stages the sidecars built in other repositories (`ra`, `rb`) from their
//      published GitHub releases, and — for `ra` — flattens the headless
//      bundle's `tools/` helpers into `binaries/` too, so `ra` finds them
//      beside its own executable.
//   5. For a macOS target, also wraps the helper in an app of its own,
//      `src-tauri/binaries/rc-computer-helper.app`, which the bundle
//      carries as `Contents/Helpers/rc-computer-helper.app` (see
//      `tauri.macos.conf.json`). macOS charges an executable's permissions to
//      the app bundle it sits in: a helper beside RecurCode in `Contents/MacOS/`
//      would hold RecurCode's — every agent's shell's — and none of its own.
//
// `rc-computer-helper` takes its trust anchors from the environment at
// compile time (`RC_COMPUTER_PEER_REQUIREMENT`): the release workflow sets
// it for the macOS builds, and a local build without it is a development
// helper that says so. Nothing here sets or defaults it.
//
// Why a separate script (not inline in beforeBuildCommand / GitHub Actions):
//   - Cross-compile in release.yml passes `--target <triple>` so we honour
//     the matrix triple rather than rebuilding for the host.
//   - Local `pnpm tauri dev` / `pnpm tauri build` invoke it without args and
//     get a host-triple build, so the externalBin lookup still finds a file.
//   - Skippable: set `RC_SKIP_SIDECAR=1` when iterating on the frontend
//     and you don't care about delegation or computer use: a development
//     RecurCode refuses a computer helper built from other sources than its own,
//     so after a change under `src-tauri/src/computer/` run this script once
//     (it is what rebuilds the helper) and restart `pnpm tauri dev`.
//
// Intentionally Node-only (no shell): runs identically on macOS, Linux,
// Windows GitHub runners.

import { execFileSync } from "node:child_process"
import {
  existsSync,
  copyFileSync,
  mkdirSync,
  chmodSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs"
import { dirname, basename, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import process from "node:process"

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const SRC_TAURI = resolve(SCRIPT_DIR, "..")
const BINARIES_DIR = join(SRC_TAURI, "binaries")
// Every sidecar in `bundle.externalBin`, in the order they are built. (On
// macOS the helper is bundled as an app instead; see `stageHelperApp`.)
const BIN_NAMES = ["rc-mcp", "rc-computer-helper"]
// The features their binary targets require (Cargo.toml).
const FEATURES = "mcp-bin,computer-helper"
const HELPER = "rc-computer-helper"
const HELPER_APP = join(BINARIES_DIR, `${HELPER}.app`)

// The helper binaries a headless `ra` bundle carries under its `tools/` folder.
// They must ship FLAT beside `ra`: `ra` probes `<exe_dir>/<name>` first and
// `<exe_dir>/tools/<name>` second (ra-agent `helper_binaries`), and Tauri's
// `externalBin` cannot carry a directory — which is why the archive's `tools/`
// is flattened here. Kept in sync with RecurAgent's
// `scripts/bundle-release.sh` TOOLS list and with `bundle.externalBin` in
// `tauri.conf.json` / `tauri.macos.conf.json` (externalBin is static, so a
// helper missing from either list is simply never bundled).
const RA_TOOLS = [
  "ra-sandbox",
  "news_fetch",
  "deep-search",
  "deep_crawl",
  "send_email",
  "account_manager",
  "voice",
  "clock",
  "weather",
  "smart_home",
]

// RecurCode's spelling of a rust target triple, matching the platform keys the
// RecurBusyBox release publishes (`rb-<platform>.*`).
const RB_PLATFORM = {
  "x86_64-pc-windows-msvc": "windows-x86_64",
  "x86_64-unknown-linux-gnu": "linux-x86_64",
  "aarch64-unknown-linux-gnu": "linux-aarch64",
  "x86_64-apple-darwin": "darwin-x86_64",
  "aarch64-apple-darwin": "darwin-aarch64",
}

// Sidecars built in OTHER repositories and published as their own GitHub
// releases. They are fetched here (never compiled in this repo) so the app
// ships them, and — on macOS — signs and notarizes them together with
// RecurCode: an executable beside the app in `Contents/MacOS/` is covered by
// the app's own signature only if it is inside the bundle at signing time.
//
// `asset` returns null for a triple the upstream does not publish, which is
// skipped with a warning rather than failing the build.
const EXTERNAL = [
  {
    name: "ra",
    repo: "icehomura/RecurAgent",
    // `ra-headless-bundle-<triple>.{zip,tar.gz}` — the HEADLESS build (kernel +
    // tools, `--no-default-features --features api`), NOT `ra-bundle-*`.
    //
    // RecurCode drives Ra over ACP, which is a JSON-RPC protocol on
    // stdin/stdout: one stray non-JSON line corrupts a frame and the session
    // dies mid-parse. The full `ra-bundle` links the terminal UI, and a UI
    // that thinks it owns the terminal is exactly the wrong tenant on that
    // channel. `ra acp` already routes its own logs to stderr, but the only
    // way to guarantee no TUI can ever reach the protocol channel is to ship a
    // build that contains no TUI code at all. The registry's download URLs
    // (`registry.rs`, AgentType::Ra) point at the same headless archives, so
    // staged and downloaded Ra are the same variant.
    //
    // The executable inside either archive is still named `ra`.
    asset: (triple) =>
      `ra-headless-bundle-${triple}${triple.includes("windows") ? ".zip" : ".tar.gz"}`,
    // Executable name inside the archive (before the `.exe` suffix).
    inner: "ra",
    // Helper binaries carried in the archive's `tools/` folder, staged flat
    // beside the sidecar so `ra` finds them (see `RA_TOOLS`).
    tools: RA_TOOLS,
  },
  {
    name: "rb",
    repo: "icehomura/RecurBusyBox",
    // `rb-<platform>.{zip,tar.gz}`; the platform spelling is RecurCode's own.
    asset: (triple) => {
      const platform = RB_PLATFORM[triple]
      if (!platform) return null
      return `rb-${platform}${triple.includes("windows") ? ".zip" : ".tar.gz"}`
    },
    inner: "rb",
  },
]

function log(msg) {
  console.log(`[prepare-sidecars] ${msg}`)
}

function die(msg) {
  console.error(`[prepare-sidecars][ERROR] ${msg}`)
  process.exit(1)
}

function parseArgs(argv) {
  const args = { target: null }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === "--target" && argv[i + 1]) {
      args.target = argv[++i]
    } else if (a.startsWith("--target=")) {
      args.target = a.slice("--target=".length)
    }
  }
  return args
}

function resolveHostTriple() {
  try {
    const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
    const line = out.split(/\r?\n/).find((l) => l.startsWith("host:"))
    if (!line) throw new Error("rustc -vV missing host: line")
    return line.replace(/^host:\s*/, "").trim()
  } catch (e) {
    die(`cannot determine host triple via rustc -vV: ${e.message}`)
  }
}

// The helper as an app: its Info.plist (identifier `app.rc.computer-helper`,
// no Dock icon), its executable, RecurCode's icon. Sealed ad hoc, so a bundle
// signed around it accepts it; release.yml signs it again with the Developer
// ID before `tauri build`.
function stageHelperApp(built) {
  const version = JSON.parse(
    readFileSync(join(SRC_TAURI, "tauri.conf.json"), "utf8")
  ).version
  const plist = readFileSync(
    join(SRC_TAURI, "macos", `${HELPER}.plist`),
    "utf8"
  ).replaceAll("{{version}}", version)
  rmSync(HELPER_APP, { recursive: true, force: true })
  const contents = join(HELPER_APP, "Contents")
  mkdirSync(join(contents, "MacOS"), { recursive: true })
  mkdirSync(join(contents, "Resources"), { recursive: true })
  writeFileSync(join(contents, "Info.plist"), plist)
  const exe = join(contents, "MacOS", HELPER)
  copyFileSync(built, exe)
  chmodSync(exe, 0o755)
  copyFileSync(
    join(SRC_TAURI, "icons", "icon.icns"),
    join(contents, "Resources", "icon.icns")
  )
  if (process.platform === "darwin") {
    execFileSync("codesign", ["--force", "--sign", "-", HELPER_APP], {
      stdio: "inherit",
    })
  } else {
    log(`not on macOS: ${HELPER_APP} is left unsigned`)
  }
  log(`helper app staged at ${HELPER_APP}`)
}

// ── Externally-built sidecars ──────────────────────────────────────────────

/** The tag whose assets we stage: the rolling `latest` pre-release, else the
 *  newest official release. `latest` is where every main push lands, so a dev
 *  build picks up the newest binaries without waiting for a version tag. */
function resolveExternalTag(repo) {
  const api = `https://api.github.com/repos/${repo}/releases`
  // Node-only fetch (no curl/wget): the script must run identically on every
  // runner. `redirect: follow` settles the api.github.com → codeload hop.
  const json = execFileSync(
    process.execPath,
    ["-e", `fetch(${JSON.stringify(api)}, {headers: {'User-Agent': 'recurcode-prepare-sidecars'}}).then(r => r.text()).then(t => process.stdout.write(t))`],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }
  )
  const releases = JSON.parse(json)
  if (!Array.isArray(releases) || releases.length === 0) return null
  const latest = releases.find((r) => r.tag_name === "latest")
  return (latest ?? releases[0]).tag_name
}

/** Download `<repo>`'s `asset` at `tag` into `dest`. Returns false on 404. */
function downloadAsset(repo, tag, asset, dest) {
  const url = `https://github.com/${repo}/releases/download/${tag}/${asset}`
  const script = `fetch(${JSON.stringify(url)}, {redirect: 'follow'}).then(async r => { if (!r.ok) { process.exit(3) } const b = Buffer.from(await r.arrayBuffer()); require('fs').writeFileSync(${JSON.stringify(dest)}, b) })`
  try {
    execFileSync(process.execPath, ["-e", script], { stdio: ["ignore", "ignore", "inherit"] })
    return true
  } catch {
    return false
  }
}

/** Extract `archive` into `dir`.
 *
 *  The extractor is chosen by format, not by platform ethno-centrism:
 *  - `.zip` → PowerShell's `Expand-Archive`. A Windows PATH also carries GNU
 *    tar (git's), which cannot read zip at all; `Expand-Archive` is the one
 *    extractor guaranteed present.
 *  - `.tar.gz` → the system `tar`, run from `dir` with a BARE name because GNU
 *    tar reads the `D:` of an absolute Windows path as a remote host
 *    (`Cannot connect to D:`). Assets are `.zip` only on Windows. */
function extractArchive(archive, dir) {
  if (archive.endsWith(".zip")) {
    const q = (s) => `'${s.replaceAll("'", "''")}'`
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `Expand-Archive -LiteralPath ${q(archive)} -DestinationPath ${q(dir)} -Force`,
      ],
      { stdio: "inherit" }
    )
  } else {
    execFileSync("tar", ["-xzf", basename(archive)], {
      cwd: dir,
      stdio: "inherit",
    })
  }
}

function stageExternalSpec(spec, target, isWindows) {
  const asset = spec.asset(target)
  if (!asset) {
    log(`${spec.name}: no published asset for ${target} — skipped`)
    return
  }
  const tag = resolveExternalTag(spec.repo)
  if (!tag) {
    log(`${spec.name}: ${spec.repo} has no releases yet — skipped`)
    return
  }

  const work = join(BINARIES_DIR, `.tmp-${spec.name}`)
  rmSync(work, { recursive: true, force: true })
  mkdirSync(work, { recursive: true })
  try {
    const archive = join(work, asset)
    if (!downloadAsset(spec.repo, tag, asset, archive)) {
      log(`${spec.name}: ${tag}/${asset} not found — skipped`)
      return
    }
    extractArchive(archive, work)

    const exe = isWindows ? `${spec.inner}.exe` : spec.inner
    // The archive is flat today; search one level down too, so a future layout
    // change does not silently stage nothing.
    const candidates = [join(work, exe)]
    for (const entry of readdirSync(work)) {
      candidates.push(join(work, entry, exe))
    }
    const found = candidates.find((p) => existsSync(p))
    if (!found) {
      log(`${spec.name}: no ${exe} inside ${asset} — skipped`)
      return
    }

    const dest = join(
      BINARIES_DIR,
      `${spec.name}-${target}${isWindows ? ".exe" : ""}`
    )
    copyFileSync(found, dest)
    if (!isWindows) chmodSync(dest, 0o755)
    log(`${spec.name} ${tag}: ${asset} → ${dest}`)

    stageAttachedTools(spec, work, target, isWindows)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

/**
 * Stage a bundle's helper binaries as flat Tauri `externalBin` siblings.
 *
 * The archive keeps them under `tools/` (older rolling releases carried them
 * at the archive root), so both layouts are accepted. Each staged file uses
 * Tauri's `<name>-<target-triple>` convention, which is what makes the bundler
 * install it beside `ra` under its bare name.
 *
 * A helper missing from the archive is a warning, not a hard failure: an
 * upstream release that predates the `tools/` layout must not block a
 * RecurCode build. The release workflow's "Verify sidecars landed" step is
 * where an incomplete set is meant to fail loudly.
 */
function stageAttachedTools(spec, work, target, isWindows) {
  if (!spec.tools?.length) return
  const nested = join(work, "tools")
  const dir = existsSync(nested) ? nested : work
  const ext = isWindows ? ".exe" : ""
  let staged = 0
  for (const tool of spec.tools) {
    const src = [join(dir, tool), join(dir, `${tool}.exe`)].find(
      (p) => existsSync(p) && statSync(p).isFile()
    )
    if (!src) {
      log(`${spec.name}: helper ${tool} not in the archive — skipped`)
      continue
    }
    const dest = join(BINARIES_DIR, `${tool}-${target}${ext}`)
    copyFileSync(src, dest)
    if (!isWindows) chmodSync(dest, 0o755)
    staged += 1
  }
  log(`${spec.name}: staged ${staged}/${spec.tools.length} helpers beside the sidecar`)
}

function main() {
  if (process.env.RC_SKIP_SIDECAR === "1") {
    log("RC_SKIP_SIDECAR=1 — skipping sidecar preparation")
    log(
      "computer use refuses a helper older than src-tauri/src/computer/; " +
        "run `pnpm tauri:prepare-sidecars` after changing it"
    )
    return
  }

  const { target: cliTarget } = parseArgs(process.argv.slice(2))
  const target =
    cliTarget || process.env.TAURI_TARGET_TRIPLE || resolveHostTriple()
  const isWindows = target.includes("windows")
  const ext = isWindows ? ".exe" : ""

  log(`target triple: ${target}`)
  log(
    `building ${BIN_NAMES.join(", ")} (--release --no-default-features --features ${FEATURES})`
  )

  // cargo build needs to run from src-tauri so it resolves the local manifest
  // and shares the swatinem/rust-cache key with other cargo invocations.
  // `--no-default-features` keeps the sidecars free of the Tauri runtime deps
  // — so they cross-compile without dragging in macOS-private-api / Linux
  // WebKit / Windows WebView2. One cargo invocation for both, so they share
  // one dependency build. Each binary target needs its own feature (see
  // Cargo.toml): off by default, so `tauri build` leaves them alone.
  execFileSync(
    "cargo",
    [
      "build",
      "--release",
      ...BIN_NAMES.flatMap((name) => ["--bin", name]),
      "--no-default-features",
      "--features",
      FEATURES,
      "--target",
      target,
    ],
    { stdio: "inherit", cwd: SRC_TAURI }
  )

  mkdirSync(BINARIES_DIR, { recursive: true })
  for (const name of BIN_NAMES) {
    const built = join(SRC_TAURI, "target", target, "release", `${name}${ext}`)
    if (!existsSync(built)) {
      die(`expected ${built} after cargo build, but it does not exist`)
    }
    const dest = join(BINARIES_DIR, `${name}-${target}${ext}`)
    copyFileSync(built, dest)
    if (!isWindows) {
      // copyFileSync preserves modes on POSIX, but be explicit for tarball
      // sources that may strip the +x bit.
      chmodSync(dest, 0o755)
    }
    log(`sidecar staged at ${dest}`)
    if (name === HELPER && target.includes("apple-darwin")) {
      stageHelperApp(built)
    }
  }

  // Then the sidecars built in their own repositories.
  for (const spec of EXTERNAL) {
    try {
      stageExternalSpec(spec, target, isWindows)
    } catch (e) {
      // Never fail the build over an upstream that is unreachable or not yet
      // published: a RecurCode without `ra`/`rb` still runs, it just has
      // fewer components installed.
      log(`${spec.name}: staging failed (${e.message}) — skipped`)
    }
  }
}

main()
