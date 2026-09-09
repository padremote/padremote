---
name: reinstall
description: Rebuild and reinstall PadRemote so the running menu-bar app serves the changes, then report what to test. Run at the end of every goal that touched web/ or desktop/, so the user can pick up their phone and try it without building anything themselves.
---

# Hand back a running app, not a diff

`desktop/build.rs` compiles `web/dist` into the binary — "the page a phone loads
is the page this binary was built from". So until the app is rebuilt, the
running menu-bar app serves the *old* page, and the user picking up their phone
sees no change and reasonably concludes the work did not land.

The connect page is the same story by a different route: `connect.html` is
`include_str!`d by `pairing.rs`, so it is inside the binary too, and editing it
without rebuilding changes nothing the user can see either.

Finish every goal by leaving a running app with the change in it.

## Before installing

Run the checks for the half you touched. Both, if you touched both.

```
cd web && npm run check && npm run build          # touched web/
PATH=$HOME/.cargo/bin:$PATH cargo test --manifest-path desktop/Cargo.toml
```

Do not install a broken build. If either fails, fix it first — reinstalling
over a failure hands the user a worse app than they had.

`cargo test` is not optional for a `desktop/` change, and it is easy to skip
because `install.sh` builds without it: the build succeeding says only that the
code compiles. The gesture engine's eleven shared fixtures live there, and they
are how you find out that a change meant to fix one gesture quietly broke
another.

Neither of these can see a class name that produced no rule — Tailwind scans
for names spelled out in full and says nothing about the ones it did not find.
That is what the `ui-review` skill is for, and it comes *before* this one: an
install is how the user tries the change, not how you find out whether it
looks right.

### The tests that touch the real cursor

Two `#[ignore]`d tests in `desktop/tests/injection.rs` post real CGEvents and
move the actual pointer, so `cargo test` skips them. They are the only thing
that can tell you whether injection works *on this Mac* rather than whether the
engine emitted the right `InputAction`, so run them after any change to
`input/macos.rs`:

```
PATH=$HOME/.cargo/bin:$PATH cargo test --manifest-path desktop/Cargo.toml \
    --test injection -- --ignored --nocapture
```

They need the Accessibility grant (they assert on it first) and they put the
cursor back where they found it. Warn the user before running them if they might
be using the machine.

### Prove the fix actually fixes it

For a bug fix, the check that matters is the one that fails without the fix.
Disable the new line, run the new test, watch it fail, put it back. It takes a
minute and it is the difference between a test that documents the fix and a test
that would have passed all along — which is most of the value of writing it.

## Install

```
cd /Users/annguyen/Documents/Github/syan-dev/padremote.com
PATH=$HOME/.cargo/bin:$PATH ./install.sh --login
```

- **`--login` is required.** Bare `./install.sh` prompts on `/dev/tty` for the
  login item and will hang, because you cannot answer it. `--login` keeps the
  login item the user already has. Use `--no-login` only if they ask.
- **Cargo is not on `PATH`** in this environment; the prefix is not optional.
- Takes a few minutes on a cold cargo cache. Run it in the background and let
  the notification wake you rather than blocking on a 600s timeout. Do not pipe
  it through `tail` to keep the output short: the pipe buffers, so the output
  file stays empty until the whole thing exits and you cannot see progress. Let
  it write in full and read the end of the file when it finishes.
- The script quits the old copy and starts the new one itself. Do not `open -a`
  or kill anything by hand — it is careful about not raising two Accessibility
  prompts, and helping breaks that.

## Then tell them what to do

Report, briefly:

1. That the app is rebuilt and running (the ●● menu-bar icon is this build).
2. **The phone must reload the page.** It is holding the old one. Pull to
   refresh, or close the tab and re-scan.
3. The one or two things to actually try, named as a user would ("open Settings
   from the phone, tap Gestures, swipe rows should blur as they move").

## The permission trap

This app is signed ad-hoc, so macOS ties the Accessibility grant to the exact
build. **Every rebuild silently loses it while the checkbox stays ticked** — the
cursor simply stops moving. If the user reports that after a reinstall, it is
this, not your change. The fix is a one-off self-signed certificate and then
`CODESIGN_IDENTITY="PadRemote Dev" ./install.sh --login`; `install.sh` prints
the steps.
