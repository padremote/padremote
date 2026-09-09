# PadRemote

A phone becomes a trackpad for a Mac. Two halves, one product:

- `desktop/` — Rust menu-bar app. Gesture engine, input injection, the
  WebSocket server, and the phone page compiled into the binary.
- `web/` — the five pages Vite builds: the trackpad (`index.html`), the full
  settings page (`config.html`), and three benches — `debug.html` for a live
  connection, `haptics.html` for a phone that won't buzz, `preview.html` for
  the press animation.
- `desktop/src/assets/connect.html` — the connect page, served by the app
  rather than built by Vite, because only the app can draw the QR.
- `docs/dev/design.md` — the UI principles *and* how the interface is built.
  Read it before changing a page.
- `docs/dev/architecture.md` — how the halves fit together.

## Two rules for finishing work

**1. Look at every UI change in a browser before calling it done.**
Use the **Claude in Chrome extension** — not headless Chrome, not CDP. A
passing check suite says nothing about whether a page is any good, and it
cannot see a class that generated nothing. Both widths, every state you
touched. See the `ui-review` skill.

**2. Reinstall at the end of every goal that touched `web/` or `desktop/`.**
`desktop/build.rs` bakes `web/dist` into the binary, so the running app serves
the old page until it is rebuilt — the user picks up their phone, sees nothing
changed, and reasonably concludes the work did not land. See the `reinstall`
skill. Then tell them the phone must reload the page it is holding.

## The interface is utility classes

Every layout, spacing, colour and type decision is a Tailwind class on the
element it applies to — in the five pages, and in the TypeScript that generates
the rest of the markup. A rule is read off the element, not hunted for in a
file somewhere else.

`web/src/theme.css` is the only hand-written stylesheet. It holds the three
things a utility class cannot be: the `@theme` tokens the utilities are
generated from, `@layer base` defaults for the form controls drawn with
pseudo-elements no class can reach, and the handful of component classes whose
*name* is the interface — `btn`, `panel`, `dot`, `tag`, `row` — because
TypeScript writes them or a check asserts them string-for-string.

The other two `.css` files are not styling and are not Tailwind entry points:
`config.css` is the SVG animation engine behind the gesture drawings, and
`style.css` is the trackpad frame's viewport-height fallback *pair*, which one
class cannot hold. There is no third. Add a stylesheet only when you can say
which of those two categories it is.

## Commands

```
cd web && npm run check      # all behaviour checks (no desktop needed)
cd web && npm run build      # tsc --noEmit, then vite build
cd web && npm run dev        # the pages on :5173
cd web && npm run mock       # a fake desktop on :8788, for looking at settings

PATH=$HOME/.cargo/bin:$PATH ./install.sh --login    # rebuild + reinstall + relaunch
PATH=$HOME/.cargo/bin:$PATH cargo test --manifest-path desktop/Cargo.toml
```

`cargo` is not on `PATH` here; the prefix is not optional. Bare `./install.sh`
prompts on `/dev/tty` and will hang — always pass `--login` or `--no-login`.

## Things that have cost time before

- **A rebuild silently drops the Accessibility grant.** Ad-hoc signing ties it
  to the exact build, so the checkbox stays ticked while the cursor stops
  moving. If that happens after a reinstall it is not your change.
- **A class name that is not written out in full never exists.** Tailwind
  builds the stylesheet by scanning `web/*.html` and `web/src/**/*.ts` for
  literal text, so a name assembled at runtime — `` `bg-${tone}` `` — produces
  nothing: no build error, no console warning, an unstyled element that
  typechecks and passes every check. State that varies is a component class
  with a whole word after it (`dot connected`, `tag mirrored`), which is most
  of why those classes exist at all.
- **The connect page is outside all of it.** `connect.html` is `include_str!`d
  by `pairing.rs` and carries its own inline copy of the palette. Tailwind
  never scans it; Vite never builds it. A utility class added there does
  nothing, and a token changed in `theme.css` leaves it behind — keep the copy
  in step by hand. Its behaviour is covered by `web/scripts/check-connect.mjs`,
  which runs the real inline script out of the real file.
- **The settings page needs a computer behind it.** It renders from the config
  the desktop sends. Use `npm run mock` rather than reinstalling to look at it:
  `http://localhost:5173/config.html?h=localhost:8788#gestures`.
- **A page served from `:5173` is a different origin from the app's port**, so a
  phone has no stored credential there and will say "Not paired". On the Mac,
  the menu bar's Settings… link carries `#k=` and works from any origin.
- **The desktop is the source of truth for the form.** The settings page is
  generated from the config the desktop sends, never hand-written — a setting
  added to the Rust struct must appear on the page without the page being
  edited.
- **A helper nobody calls looks exactly like a fix.** `sync_from_system` was
  written to re-read the cursor "in case the user touched the real trackpad",
  and was called from nowhere at all — so the bug it names was live for as long
  as it existed, and reading the source said the opposite. `grep` for the call,
  not the definition, and leave behind a test that asserts the *call* happens;
  a lone `pub fn` compiles, passes every check, and warns about nothing.

## House style

Comments explain *why*, at length, and often name the bug that made the code
look the way it does. That is deliberate and it is the standard — match the
density of the file you are editing rather than stripping it back. Prose in
comments and docs uses plain words over jargon; look at `web/src/gesturepad.ts`
or `desktop/src/net/devices.rs` for the register. That applies to markup too: a
long utility string is where a comment goes, saying what the element is for,
not restating the classes.

Behaviour changes to a page come with a check in `web/scripts/check-*.mjs`.
Those use a hand-rolled DOM stub, not jsdom, so page code must stay inside
`getElementById`, `createElement(NS)`, `append`, `querySelectorAll`,
`classList`, `dataset` and `setAttribute`.
