# `tools/proto` — milestone-0 prototype (throwaway)

Proves and tunes the gesture engine against a live cursor before the Rust app
exists (plan.md section 5). **Its only durable outputs** are the tuned numbers in
`desktop/config.default.json` and the recorded touch streams in
`desktop/tests/fixtures/`. The code itself is disposable.

## Setup

```sh
python3 -m venv .venv
.venv/bin/pip install websockets pyobjc-framework-Quartz pyobjc-framework-ApplicationServices
```

## Run

```sh
.venv/bin/python -m proto              # http://localhost:8080/page.html + ws://localhost:8787
.venv/bin/python -m proto --dry-run    # recognise gestures, print actions, inject nothing
.venv/bin/python -m proto --record out.json   # save the raw touch stream as a fixture
```

macOS will refuse to inject input until the terminal running this has
**Accessibility** permission (System Settings → Privacy & Security →
Accessibility). The app detects that and prints the deep link rather than
failing silently.

## Driving it

The page reports **touch** pointers only. That is deliberate: opened on the very
computer it controls, any cursor it moves passes back over the page as a mouse
event and feeds itself — a runaway loop. So:

- **A real phone** on the LAN — point it at `http://<your-ip>:8080/page.html`.
- **A trackpad**, for recogniser work — add `?mouse=1` to the URL, and pair it
  with `--dry-run` so nothing is injected and the loop cannot form.
- **Synthetic streams** — `make_fixtures.py` builds every v1 gesture as an exact,
  reproducible stream, checks the recogniser against it, and writes the fixtures
  the Rust test suite asserts against. This is the fastest loop of the three:

```sh
.venv/bin/python make_fixtures.py
```

## Layout

| file | role |
|---|---|
| `proto/recognizer.py` | the gesture state machine — ported to `desktop/src/gesture/` |
| `proto/inject_macos.py` | Quartz CGEvent injection — mirrored in `desktop/src/input/macos.rs` |
| `proto/server.py` | static page + WebSocket + the loop that wires them together |
| `proto/page.html` | throwaway ancestor of `web/src/surface.ts` |
| `make_fixtures.py` | synthetic gesture suite and fixture generator |
