---
name: ui-review
description: Look at a UI change in a real browser with the Claude in Chrome extension and judge it, instead of declaring it done from the code. Use after any change to a page's markup or classes - web/*.html, the TypeScript that generates markup, web/src/theme.css, desktop/src/assets/connect.html - or anything that alters layout, spacing, animation or navigation, and before reporting a UI goal finished.
---

# Look at it before you call it done

A stylesheet that compiles and a check suite that passes say nothing about
whether the page is any good. Every UI change in this repo is looked at in a
real browser, by you, at both widths, before it is reported as finished.

Since the interface became utility classes this stopped being only a taste
question. Tailwind generates the stylesheet by scanning `web/*.html` and
`web/src/**/*.ts` for class names spelled out in full, so a typo, a name
assembled from a variable, or a class written in a file the scanner does not
read produces **no rule at all** — no build error, no console warning, no
failing check. The browser is the only place that shows up. See "A class that
generated nothing" below.

Use the **Claude in Chrome extension** (`mcp__claude-in-chrome__*`). Not
headless Chrome, not CDP device metrics, not a screenshot library — the user
asked for the extension so they can watch and take over.

## 1. Put something behind the page

Which page you changed decides what has to be running:

| Page | What it needs |
|---|---|
| `config.html` — settings | `npm run mock` + `npm run dev`. It renders itself from the config the desktop sends; with nothing behind it you are screenshotting an error message |
| `preview.html` — press animation | `npm run dev` alone. It runs the real renderer on the computer |
| `haptics.html`, `debug.html` | `npm run dev`; `debug.html` wants a real app to say anything |
| `index.html` — the trackpad | The real app. The mock speaks only the settings channels |
| `connect.html` — pairing | The real app, on its own port. It is served by `pairing.rs`, not by Vite, so `:5173` does not have it at all |

```
cd web
npm run mock &     # canned /config and /devices on :8788, no pairing needed
npm run dev &      # the pages on :5173
```

## 2. Load the tools in one call

```
ToolSearch: select:mcp__claude-in-chrome__tabs_context_mcp,mcp__claude-in-chrome__navigate,mcp__claude-in-chrome__computer,mcp__claude-in-chrome__resize_window,mcp__claude-in-chrome__tabs_close_mcp,mcp__claude-in-chrome__read_console_messages
```

`tabs_context_mcp` first, then `tabs_create_mcp` — never reuse a tab from
another session.

## 3. Walk every state you changed

The settings page is four hash routes in one document:
`http://localhost:5173/config.html?h=localhost:8788#<page>` — `home`,
`basics`, `gestures`, `advanced`. The connected devices are on `home`, not a
route of their own.

Both widths, every time:

- **Desktop**: `resize_window` 1440x900.
- **Phone**: `resize_window` 480x900. Chrome will not go much narrower than
  ~480 on macOS; that is below the 620px breakpoint, which is what matters.

The theme also has height breakpoints — `short`, `tiny`, `flat` — that a wide
window never shows. If you touched the trackpad frame or anything that has to
survive a landscape phone, resize to something like 900x420 as well.

For an **animation** change, one screenshot is not evidence — the loop is 3s
and you will catch a still frame. Take three or four `zoom` captures of the
same region in a row and look at what differs between them.

## 4. Judge it, and say what is wrong

Screenshotting is not reviewing. Look for, and fix before reporting:

- **A class that generated nothing.** An element sitting at browser defaults —
  unpadded, unrounded, full-width, the wrong grey — when the markup clearly
  asks for otherwise. Read the class off the element and grep for it spelled
  out in full in `web/`; if the only place it appears is inside a template
  literal, that is the bug. The fix is a component class in `theme.css` with a
  whole word for the state, the way `dot connected` and `tag mirrored` work.
- **A change to `connect.html` that did nothing.** That page carries its own
  inline copy of the palette and is not scanned or built. Utility classes do
  not work there; a `theme.css` token change does not reach it.
- Things that do not share a measure — a heading at one margin and the content
  it heads at another.
- Grid items that move when a sibling is hidden. Place by `grid-column` /
  `grid-row`; `order` alone lets auto-flow slide the remaining columns.
- Dead vertical space. A page of controls floating at the top of an empty
  window is not "fine on a big screen".
- Colour that overstates: red is a fault, not "this phone is switched off".
- Text centred under a centred thing, or left under it. Pick one.
- Touch targets under 44px, and input text small enough to make a phone zoom
  when it takes focus. Both are theme defaults, so an element missing them is
  usually one that opted out by accident.

Say plainly what you found and what you changed. "It looks good" after one
screenshot is the failure this skill exists to prevent.

## 5. Clean up

Close the tab you opened (`tabs_close_mcp`), restore the window to a sensible
size, and `pkill -f mock-desktop.mjs`. Leave `npm run dev` if the user is still
working.
