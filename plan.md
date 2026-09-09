# PadRemote — Phone Touchscreen as a Wireless Trackpad

**v1 target OS: macOS.** The phone page, protocol, networking, pairing and gesture *recognizer* are OS-independent; only the input-injection backend is platform-specific. Windows and Linux are planned as additional backends behind the same engine (section 16), so nothing in the architecture is Mac-only.

Domain: **padremote.com** (single domain; no other TLDs needed for v1).
- `https://padremote.com` — landing page; phone web app served at `/go` (`https://padremote.com/go`)
- No signaling server or any always-on backend in v1: the phone connects to the desktop app directly over the local network. The static page can even be self-hosted by the desktop app for a fully offline product (section 16).
- Config paths and identifiers use the `padremote` prefix.

## 1. Goal

Turn the phone's touchscreen into a wireless trackpad for the computer, with the feel of a built-in laptop trackpad. The computer shows a QR code; the user scans it with the phone's camera app, which opens a web page; that page is a blank touch surface. Finger movements and multi-touch gestures on the phone drive the computer's cursor, clicks, scrolling, dragging and zoom. Only the **computer** has an installed app; the phone runs a web page with nothing to install.

There is **no camera, no computer vision, no hand tracking** in this design. The phone's operating system reports touch positions directly; the work is turning those touches into operating-system input events. v1 ships on macOS; the same app targets Windows and Linux with a different injection backend (section 16).

**v1 gesture set (all required):** relative cursor movement, tap = left click, two-finger tap = right click, two-finger drag = scroll, pinch = zoom, tap-and-drag and press-and-drag = drag. Movement is **relative** (like a trackpad), not absolute.

## 2. Why this design

- Touch input is native to every phone browser and is pixel-accurate; there is no recognition error, no lighting dependency, no arm fatigue.
- The macOS trackpad gesture vocabulary maps almost one-to-one onto touch events, so the product feels familiar immediately.
- Bytes per gesture instead of a neural network per frame: lower latency, negligible battery use.
- The genuinely hard part is not input, it is (a) suppressing the mobile browser's own gestures so the surface feels like a trackpad and not a web page, and (b) injecting scroll and especially zoom into macOS convincingly.

## 3. System overview

```
┌──────────── Phone: web page (any modern browser) ────────────┐
│ Full-screen touch surface (Pointer Events)                    │
│  → raw touch points (id, x, y, phase)                         │
│ WebSocket client · Wake Lock · localStorage pairing           │
│ Full-screen PWA · touch-action:none · preventDefault          │
└───────────────────────────────┬───────────────────────────────┘
                                │ WebSocket (wss) over local Wi‑Fi — direct, no server
                                ▼
┌──────────── Computer: desktop app (Rust, one binary/OS) ──────┐
│ WebSocket server (rustls, self-signed) · pairing store        │
│ Gesture engine (touch → intent)  ── shared, OS-agnostic       │
│ Input injector (enigo + per-OS): move/click/scroll/drag/zoom  │
│ Tray icon: QR, status, settings · guided first-run permission │
└───────────────────────────────────────────────────────────────┘

  Static page hosting: padremote.com (CDN) serves landing + touch app at /go
  (first load only; can be self-hosted by the desktop app for fully offline use)
```

Design rules:
- The phone page is intentionally dumb: it reports **raw touch points**, nothing more. It does not decide what a tap or a scroll is.
- **All gesture interpretation lives on the desktop app**, driven by `config.json`, so behaviour and sensitivity can be tuned without redeploying the page.
- v1 transport is a **plain WebSocket over the local Wi‑Fi** (both devices are on the same network for a desk trackpad), so there is **no hosted server and no signaling** to run. WebRTC + a signaling worker is kept as a later upgrade for the cross-network case only (section 16).
- The desktop app is one **compiled Rust binary** per OS; the gesture engine and config are shared across macOS/Windows/Linux, and only the input-injection backend differs.
- The protocol is client-agnostic; a native phone app could replace the web page later without touching the desktop side.

## 4. Repository layout (monorepo)

```
padremote/
├── PLAN.md
├── protocol/              # shared message schema + version
│   └── v1.schema.json
├── desktop/               # cross-platform desktop app (Rust, one binary per OS)
│   ├── src/
│   │   ├── main.rs        # tray icon, first-run/permission flow, lifecycle
│   │   ├── net.rs         # local WebSocket server, discovery, pairing store
│   │   ├── gesture/       # touch→intent recognizer, config, accel curve (shared, OS-agnostic)
│   │   └── input/         # injection backend: macos.rs / windows.rs / linux.rs (enigo + per-OS)
│   ├── tests/             # recognizer tests on recorded touch streams
│   └── packaging/         # .dmg (mac), .msi/.exe (win), install.sh + .deb (linux)
├── web/                   # phone page (Vite + TypeScript, no framework)
│   ├── src/
│   │   ├── surface.ts     # pointer/touch capture, gesture-suppression
│   │   ├── net.ts         # local WebSocket client, discovery, reconnect
│   │   ├── pairing.ts     # session code parsing, localStorage
│   │   └── ui.ts          # status dot, hints, settings sheet
│   └── public/            # PWA manifest, service worker, icons
└── tools/
    └── proto/             # Phase 0 throwaway prototype (Python): local touch source → cursor
```

Note: `mac/` (Swift) and `signaling/` (Cloudflare Worker) from earlier drafts are gone. The desktop app is Rust; there is no signaling service in v1.

## 5. Tech stack

### Phone web page
- Vite + TypeScript, plain DOM (the UI is a full-screen surface plus a status dot and a settings sheet).
- Input: **Pointer Events** (`pointerdown/move/up/cancel`) with `getCoalescedEvents()` for full-rate movement; touch points identified by `pointerId`.
- Gesture suppression: `touch-action: none` on the surface, `preventDefault()` on all pointer/touch/gesture events, `user-select: none`, `-webkit-touch-callout: none`, disable double-tap zoom, disable iOS Safari edge-swipe where possible, full-screen standalone PWA to remove browser chrome.
- `navigator.wakeLock.request("screen")`, re-requested on `visibilitychange`.
- Transport: a single **WebSocket** (`wss://`) to the desktop app on the local network. One connection carries two logical streams — batched touch-move samples and discrete events (tap, gesture start/end, control) — distinguished by message type. (WebSocket is reliable/ordered; acceptable on a LAN. WebRTC's unreliable channel is a later optimization.)
- Installable PWA (manifest + service worker) so "Add to Home Screen" gives an app icon and offline load.
- Hosted as static files on Cloudflare Pages at `padremote.com`; app at `/go`. In v1 the page connects to the desktop app directly; no other server is contacted after the page loads.

### Desktop app (Rust, cross-platform; v1 ships macOS 13+)
- **Rust**, one self-contained compiled binary per OS — no runtime for the user to install (the "installs like Ollama" bar). Windows and Linux reuse everything below except the injection backend.
- Tray/menu-bar UI: `tray-icon` + a minimal settings window (`tao`/`muda`, or a small `egui`/Tauri shell if a richer window is wanted). QR rendered with the `qrcode` crate.
- Local server: `tokio` + `tokio-tungstenite` WebSocket listener on the LAN, advertised for discovery (see section 7). TLS via `rustls` with a self-signed cert (needed because the phone page is served over HTTPS and must reach `wss://`).
- Input injection: **`enigo`** as the cross-platform base (move, click, scroll, key events on macOS/Windows/Linux), with a thin per-OS module for anything `enigo` doesn't cover:
  - Move: relative cursor motion from accumulated deltas.
  - Click / right-click / middle-click / drag: button press/release with click-state for double-click.
  - Scroll: pixel/line scroll; smooth pixel scroll where the OS supports it.
  - Zoom: **hardest everywhere.** No OS exposes a universal synthetic magnify event. v1 uses app-level zoom (`⌘/Ctrl =` `⌘/Ctrl -`, or `Cmd/Ctrl`+scroll where honoured). Native magnify-gesture synthesis (`kCGEventGesture` on macOS, etc.) is a later spike. The recognizer emits clean zoom data regardless; only injection is approximate.
- Permissions: macOS needs **Accessibility** (`AXIsProcessTrustedWithOptions`); the app must detect it's missing and run the guided first-run flow in section 11.1. (Windows: none for `SendInput`. Linux: user in the `input` group for `uinput`, or X11 `XTEST`.)
- Config: `~/Library/Application Support/PadRemote/config.json` on macOS (`%APPDATA%`/`~/.config` on Win/Linux), hot-reloaded.
- Tests: Rust unit/integration tests, recognizer driven by recorded touch-point sequences.

### Signaling service
- **None in v1.** The phone connects to the desktop app directly over the LAN WebSocket. A signaling worker is only introduced if/when WebRTC cross-network support is added (section 16).

### Phase 0 prototype (throwaway)
- `tools/proto` (Python + `pynput` or `pyautogui`): reads touch points from a browser page on the same machine over `ws://localhost`, or from recorded JSON, and drives the cursor. Fastest way to build and tune the recognizer and to prove scroll/zoom injection before writing the Rust app. Explicitly disposable — its only output is tuned numbers copied into `config.json`.

## 6. Network usage by stage (online vs local)

Legend: **ONLINE** = crosses the internet. **LOCAL** = stays on the user's Wi‑Fi.

In v1 only the page download touches the internet. Everything about the actual connection and control is local, because there is no signaling server.

### Stage 1 — First load (ONLINE, once)
```
Phone ──HTTPS──▶ padremote.com/go (CDN): page + JS (small, no ML model)     Desktop: not involved
```
Service worker caches the page; later launches from the home-screen icon load offline. Once cached, even the first stage is local.

### Stage 2 — Connect (LOCAL)
```
Phone ──scans QR──▶ reads desktop's LAN address + pairing secret
Phone ──WSS──▶ desktop app on the LAN   (TLS handshake + auth, well under a second)
```
The QR contains the desktop app's local address and a pairing secret, so the phone connects straight to it. No internet, no third party.

### Stage 3 — Live session (LOCAL only)
```
Phone ══WebSocket over Wi‑Fi══▶ Desktop app   (touch points, tens of bytes per move)
Desktop ──status──▶ Phone
```
No cloud component at all. An internet outage never affects control.

### Stage 4 — Different networks (NOT in v1)
Phone on mobile data while the desktop is on Wi‑Fi needs a public rendezvous and NAT traversal, i.e. WebRTC + signaling (+ possibly a relay). This is deliberately out of scope for v1; the desk use case is same-Wi‑Fi. See section 16 for the upgrade path. For now the page tells the user "connect your phone to the same Wi‑Fi as your computer" if it can't find the desktop.

### Summary
| Stage | Internet needed | Duration | Crosses internet | Hosted by you |
|---|---|---|---|---|
| 1 First load | Yes, first time only | Seconds | Page + JS | Static site (or self-host) |
| 2 Connect | No | < 1 s | Nothing | — |
| 3 Live session | No | Whole session | Nothing | — |
| 4 Cross-network | (v2 only) | — | — | — |

You host **only a static page**, and even that can be self-hosted by the desktop app for a fully offline product (section 16). There is no always-on server to run or pay for in v1.

## 7. Connection & pairing flow

The QR carries the desktop app's own LAN address, so the phone connects directly with no discovery service.

### First run
1. Desktop app generates a **pairing secret** `P` (128-bit) and a self-signed TLS cert (once, stored). It starts the WebSocket server on the LAN and learns its own address(es).
2. Desktop shows a QR: `https://padremote.com/go#h=<lan-ip:port>&p=<P>&cert=<cert-fingerprint>&n=<computer name>`. The secret and fingerprint are in the URL fragment, so they never leave the phone.
3. User scans with the phone camera app → page opens → the surface appears.
4. Page opens `wss://<lan-ip:port>`, pins the cert fingerprint from the QR, and authenticates by proving it knows `P` (HMAC challenge). Mismatch → desktop drops the connection.
5. On success: desktop stores the phone as paired; page stores `P`, the address, cert fingerprint and computer name in `localStorage`.

Robustness: the QR may list several candidate addresses (Wi‑Fi, Ethernet) and the page tries them in order. If the desktop's LAN IP changes later, reconnection falls back to a small discovery step (mDNS/`_padremote._tcp` where available, else the user re-scans a fresh QR — one tap from the tray).

### Subsequent runs (no QR)
- Page (opened from the home-screen icon) reads the stored address + `P`, reconnects, and authenticates silently. Status dot goes green.
- If the stored address fails (IP changed / different network): try discovery, then show "Open PadRemote on your computer and scan again", retrying with backoff (1 s → 10 s).

### Reconnect / reset
- Auto-reconnect with backoff on socket close or network change; the desktop releases any held button the instant the socket drops (no stuck drag).
- Settings → "Unpair" regenerates `P`, invalidating stored phones; next connect needs a new QR scan.

## 8. Protocol (v1)

One **WebSocket** carries all messages. Two logical message kinds share it: batched touch-move samples (frequent, small) and discrete control/event messages (tap, gesture edges, settings, status). Because a WebSocket is reliable and ordered, a finger-up can never be lost, so no separate mirroring is needed.

Coordinates are normalized 0–1 over the surface, plus the surface's CSS pixel size so the desktop can convert to physical deltas. The phone sends **raw touches only**; the desktop decides intent.

Control/event messages (JSON, text frames):
```json
{ "t":"auth", "hmac":"<base64 HMAC(P, challenge)>" }
{ "t":"welcome", "v":1, "surface":{"wpx":390,"hpx":716,"dpr":3} }    // phone → desktop at connect
{ "t":"state", "gesture":"idle|move|scroll|drag|zoom", "fingers":0 } // desktop → phone, for UI
{ "t":"settings", "sensitivity":1.0, "naturalScroll":true }          // phone → desktop, from settings sheet
{ "t":"error", "code":"badAuth|version" }
```

Touch batches (phone → desktop, binary frames, little-endian). One frame may batch several coalesced samples captured since the last send:
```
u8  version = 1
u8  count               // number of touch samples in this frame
per sample:
  u32 t_ms              // performance.now() at the sample
  u8  pointerId
  u8  phase             // 0 down, 1 move, 2 up, 3 cancel
  f32 x, f32 y          // normalized 0–1 on the surface
```
Move samples are batched at the display frame rate to keep frame count low. Binary keeps each sample at 14 bytes; a JSON variant behind a debug flag aids inspection.

## 9. Gesture engine (desktop app)

Pure, deterministic, testable. Consumes decoded touch samples, tracks active pointers, emits `InputAction`s. This is the core of the product.

### 9.1 Pointer bookkeeping
- Maintain the set of active pointers with their start time, start position, current position and path length.
- Classify the **intent of a gesture** by the number of fingers that go down within a short window (≈60 ms) and their motion, then commit to that intent until all fingers lift (so a scroll doesn't turn into a cursor jump when one finger lifts slightly early).

### 9.2 Cursor movement (relative)
- One finger moving → cursor moves by the finger's delta.
- Convert normalized delta → surface pixels → apply a **pointer-acceleration curve** (macOS-like: slow finger = fine control, fast finger = more travel). Curve parameters in config; default a smoothed quadratic on speed.
- Light smoothing (One Euro or small EMA) to remove sensor noise without adding lag.
- Emit a cursor-move event only when the accumulated delta ≥ 1 px.

### 9.3 Tap → click
- A **tap** = one finger down and up within `tapMaxMs` (default 200 ms) and movement < `tapMaxPx` (default 10 px).
- Tap → left click at the current cursor position. Two taps within `doubleTapMs` (default 300 ms) → double-click.
- **Two-finger tap** → right click. **Three-finger tap** → middle click (config).

### 9.4 Scroll (two-finger drag)
- Two fingers moving together → scroll. Scroll delta from the average finger movement.
- Pixel scroll events (via the injection backend); support momentum by continuing to emit decaying scroll events after lift if the fingers were moving fast (optional, config `momentum`).
- `naturalScroll` (default true) matches macOS direction; user-togggleable in the settings sheet.

### 9.5 Drag
- **Tap-and-drag**: tap, then within `doubleTapMs` put the finger down again and move → left button held, cursor follows, release on lift (macOS "tap-and-a-half").
- **Press-and-drag**: one finger held stationary > `pressMs` (default 250 ms) then moving → same. Config chooses which of these is enabled (default both).

### 9.6 Zoom (pinch)
- Two fingers whose distance changes → zoom. Emit zoom based on the change in finger distance (`newDist/oldDist`).
- Injection is the hard part (see 5): v1 maps zoom to the reliable app-zoom path (`⌘ =` / `⌘ -`) or magnify events if the spike succeeds. The recognizer output is clean regardless; only the injection backend is approximate.
- Disambiguation: two fingers moving in parallel → scroll; two fingers changing distance beyond a threshold → zoom; commit once decided.

### 9.7 Multi-finger swipes (config, optional in v1)
- Three-/four-finger horizontal swipe → spaces / `⌃→` `⌃←`; up → Mission Control (`⌃↑`). Wired through config; off by default in v1.

### 9.8 State machine
```
IDLE ─1 down─▶ PENDING ─move─▶ MOVING ─────────────────┐
                       └hold>press─▶ DRAG              │
     ─2 down─▶ TWO_PENDING ─parallel move─▶ SCROLL     │ all up
                          └distance change─▶ ZOOM      │
                          └quick up (no move)─▶ RIGHTCLICK
IDLE ─1 down/up quick─▶ TAP→click                      │
any ◀──────────────────────────────────────────────────┘  (all fingers up → release held buttons → IDLE)
```
Losing the connection or receiving `cancel` releases every held button immediately (no stuck drag).

### 9.9 Config (`config.json`, default)
```json
{
  "version": 1,
  "sensitivity": 1.0,
  "accel": { "curve": "quadratic", "gain": 1.0 },
  "tap": { "tapMaxMs": 200, "tapMaxPx": 10, "doubleTapMs": 300, "pressMs": 250 },
  "scroll": { "natural": true, "momentum": true, "speed": 1.0 },
  "zoom": { "enabled": true, "backend": "appZoom", "threshold": 0.05 },
  "bindings": {
    "oneTap": "leftClick",
    "twoFingerTap": "rightClick",
    "threeFingerTap": "middleClick",
    "threeFingerSwipe": "none",
    "fourFingerSwipe": "none"
  }
}
```
**v1 requirement:** the full set (move, tap-click, two-finger scroll, right-click, drag, zoom) works and is tuned. Swipes are wired through the same path but default to `none`. Config is hot-reloaded.

### 9.10 Input injection notes (macOS)
- Post events to `.cghidEventTap`. Keep a single virtual cursor position accumulator; warp + moved event each frame.
- Scroll: prefer `scrollWheelEvent2` in pixel units for smoothness.
- Right button and click state via `mouseEventClickState`.
- Zoom backend is pluggable so a better magnify implementation can drop in later.

## 10. Phone page requirements

- Full-screen surface with a subtle status dot (connected / waiting / disconnected), computer name, and a small "settings" affordance (sensitivity slider, natural-scroll toggle).
- Suppress every native browser gesture on the surface: page never scrolls, zooms, selects, shows a callout menu, or triggers Safari edge navigation.
- Report touches at display rate using coalesced pointer events; drop nothing on finger-up/cancel (mirror on `ctrl`).
- Keep the screen awake while connected; release wake lock and stop reporting when hidden; resume on return.
- Optional haptics on tap/click via the Vibration API where supported (Android; iOS Safari lacks it).
- Works in iOS Safari 16.4+ and Android Chrome 110+. Test iOS Safari first (strictest about gesture suppression and full-screen).
- Installable PWA; no analytics, no cookies, no third-party scripts.

## 11. Desktop app requirements

- Tray/menu-bar icon states: unpaired / waiting for phone / connected / active.
- "Pair phone…" opens a small window: QR + computer name; QR refreshes if the LAN address changes.
- Settings: sensitivity, natural scroll, zoom backend, tap timings, enable swipes, open config file, unpair, launch at login, record touch stream to file (for tests), latency readout.
- Runs quietly in the tray; optional launch at login so the phone reconnects without the user opening anything.

### 11.1 Install & first-run experience (the "installs like Ollama" bar)

The whole point of choosing a single compiled Rust binary is that installation is trivial and familiar. Required experience per OS:

- **macOS:** distribute a `.dmg` — drag the app to Applications, open it. The build is **Developer ID signed and notarized** so Gatekeeper shows no "unidentified developer" block. (Not the Mac App Store: input injection needs Accessibility, which sandboxed App Store apps can't have.) Optional Homebrew cask (`brew install --cask padremote`) for the CLI-inclined.
- **Windows:** a signed `.msi`/`.exe` installer, click-through, runs in the tray. Code-signing certificate to avoid SmartScreen warnings.
- **Linux:** a `curl -fsSL https://padremote.com/install.sh | sh` one-liner plus a `.deb`/AppImage, matching Ollama-style expectations.

**One unavoidable extra step vs Ollama:** because the app controls the cursor, macOS requires the user to grant **Accessibility** permission (Ollama never asks, because it doesn't touch input). This cannot be removed — the OS owns it — but it must be made painless:
- On first launch, detect the missing permission and show a single, clear screen: one sentence of explanation + a button that deep-links straight to System Settings → Privacy & Security → Accessibility (`x-apple.systempreferences:...`).
- Detect the moment permission is granted and proceed automatically; no restart if avoidable.
- Never inject events silently before permission exists; show the guidance instead.
- (Windows: no prompt needed for `SendInput`. Linux: if using `uinput`, detect missing `input`-group membership and print the exact one-line fix.)

Acceptance for install: a non-technical user gets from "download" to "cursor moves from my phone" without reading documentation, with the only friction being the single OS permission tap on macOS.

- Auto-update: optional (Sparkle-style on macOS, or a built-in version check that points to the latest installer). Not required for v1.

## 12. Security & privacy

- No camera, no microphone, no video ever. Only touch coordinates on the pairing surface are sent.
- Traffic is **TLS-encrypted** (WebSocket over `rustls` with the desktop's self-signed cert); the phone pins the cert fingerprint from the QR, and the pairing secret `P` authenticates the phone via HMAC challenge, so another device on the same Wi‑Fi cannot connect or read touches.
- No signaling server, no cloud, no accounts, no analytics — after the page loads, all traffic is device-to-device on the LAN.
- The pairing secret `P` lives only in the QR fragment and in each side's local storage; it never appears in a URL sent to any server.
- Unpairing on the desktop invalidates all paired phones.

## 13. Milestones

| # | Milestone | Deliverable | Done when |
|---|-----------|-------------|-----------|
| 0 | Injection core | `tools/proto` (Python): localhost touch page → cursor | Move, click, right-click, drag, pixel-scroll and a zoom backend all work from a `ws://localhost` touch page on one machine; timings/curve recorded in `config.json`. No HTTPS, no phone yet. |
| 1 | Rust recognizer + injection | Rust app: full gesture engine + `enigo`/per-OS injection, fed by a local fake client | Recognizer unit tests pass on recorded touch streams; all v1 gestures fire from replay; runs as a tray app |
| 2 | Touch page + local link | `web/` surface page connecting to the Rust app over `wss://` on the LAN | End-to-end control on a real phone over local Wi‑Fi: cursor + tap + scroll feel right; < 50 ms touch-to-cursor |
| 3 | QR pairing + reconnect | QR carries LAN address + secret; TLS + HMAC auth; auto-reconnect | Fresh phone: scan QR → trackpad works in < 15 s. Reopen next day → connects silently. Wi‑Fi toggle → reconnects < 3 s. IP-change fallback works. |
| 4 | Full gesture polish | Zoom disambiguation, drag variants, momentum scroll, settings sheet, hot-reload config | All v1 gestures reliable; sensitivity + natural-scroll adjustable live |
| 5 | Ship (Ollama-grade install) | Signed/notarized `.dmg` (+ `.msi`/install.sh scaffolding), guided first-run permission, PWA install, deploy static page | A non-technical user goes download → cursor-from-phone with no docs, only the single macOS permission tap |

## 14. Acceptance criteria (v1)

- Cursor movement feels like a trackpad: fine control when slow, reach across the screen when fast; no jitter at rest; perceived lag ≤ ~1 frame on same Wi‑Fi.
- Tap-click, two-finger right-click, two-finger scroll, tap/press drag all work reliably (≥ 98 % of clear attempts) with < 2 % false triggers in a 5-minute session.
- Scrolling is smooth (pixel-level), correct direction, with working natural-scroll toggle.
- Zoom changes zoom level in a common app (browser, Preview/Photos) via the chosen backend; approximate injection is acceptable and documented.
- Losing the hand/connection never leaves a button or drag stuck.
- Reconnects within 3 s after a network interruption, no user action.
- Verified on one iPhone (Safari) and one Android (Chrome) against one computer (macOS for v1).

## 15. Local development (before padremote.com exists)

The page URL is the only external dependency, and even it can be served locally. All addresses are configuration, so switching local → production is a one-line change.

### Milestones 0–1 — one machine, no HTTPS, no phone
Serve the touch page on `http://localhost:5173` and open it **on the same computer** (the desktop app and the page on one machine). `localhost` is a secure context, so touch/pointer events and `ws://localhost` work with no certificate. This is where the recognizer and all injection (move/click/scroll/drag/zoom) get built and tuned. Fully offline.
```
cd web && npm run dev            # page on http://localhost:5173
cargo run -p padremote           # desktop app; listens on ws://127.0.0.1
```

### Milestone 2+ — real phone on the LAN (needs local HTTPS)
The phone's browser needs the page over **HTTPS** (for camera-app QR links and PWA features) and must reach the desktop app over **`wss://`**. Use a locally-trusted cert; still no server.
```
brew install mkcert && mkcert -install
mkcert mycomputer.local 192.168.1.10        # cert for the desktop's LAN name/IP
cd web && npm run dev -- --host --https     # page over https on the LAN
# desktop app loads the same mkcert cert for its wss listener (dev flag)
```
Install the mkcert root CA on the phone once (iOS: install profile, then Settings → General → About → Certificate Trust Settings; Android: Settings → Security → install CA certificate). Then browse to the page from the phone, or scan the desktop app's QR. Everything stays on the LAN; no internet required.

Production note: the shipped desktop app generates its own self-signed cert and the phone pins its fingerprint from the QR, so end users never touch mkcert — the cert-trust dance is a **developer-only** convenience for loading the page from `vite` during development. The static page is hosted at `padremote.com` only so users have a URL to load once; it can also be embedded in and served by the desktop app for a fully offline product.

### Config
Point the page at the desktop app via `.env.local` (`VITE_DESKTOP_WSS=wss://mycomputer.local:PORT`) during dev; in production the address comes from the QR, not from config.

## 16. Later / open questions

- Better zoom injection (synthesized magnify gesture events) if the v1 backend proves too coarse.
- Multi-finger swipe gestures (spaces, Mission Control, app switching) enabled by default.
- On-screen buttons/modes on the phone (e.g. a dedicated right-click zone, a scroll strip) for users who prefer them.
- Keyboard: a text field on the phone that sends keystrokes to the computer.
- Cross-network use: add WebRTC + a signaling service (and a TURN relay for strict networks) so the phone can control the computer over the internet, not just the same Wi‑Fi. This is the main thing v1 gives up by using a plain local WebSocket.
- Fully offline distribution: the desktop app serves the page itself over its self-signed HTTPS, so there is no hosted page at all; the phone trusts the cert once. Removes the last internet dependency (first page load).
- Windows and Linux desktop apps: reuse the phone page, protocol, pairing and the entire gesture recognizer unchanged; implement a new injection backend only. Windows: `SendInput` for move/click/scroll and wheel, app-zoom via Ctrl+scroll or `keybd_event` Ctrl+/−. Linux: `uinput` (Wayland-friendly) or `XTEST`/`xdotool` on X11. Zoom faces the same "no universal magnify event" limitation as macOS and uses the app-zoom fallback. The `gesture/` engine and `config.json` are shared across all three platforms; only `input/` differs. (`enigo` already covers much of this, so these are mostly packaging + permission work.)
- Optional native phone app for background operation and haptics on iOS, speaking the same protocol.