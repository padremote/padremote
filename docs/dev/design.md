# Design

PadRemote should feel like a simple wireless trackpad. The touch surface is the
main task; settings explain how to adapt it; troubleshooting stays available
without filling the everyday interface with technical readings.

## Priorities

- **Pair in two steps.** Start with a short introduction and a QR code. Keep
  the manual link and diagnostics inside “Can’t scan?”. Advance only after
  an authenticated trackpad connects, then show “Move a finger on the
  trackpad” and Settings & gestures. Keep that guide visible on disconnect
  with a reconnect message; offer the QR again for another device.
- **Make the next action obvious.** Use visible, labelled controls for Settings,
  Gestures, Full screen and returning to the trackpad. Keep navigation usable
  before a computer connects.
- **Show the gesture, then name it.** Every row carries a small drawing of the
  pad with a finger on it for each one the gesture takes, and the selected
  gesture is demonstrated in the middle of the page. Words are exact and slow; a
  picture answers "which one is that?" at a glance. The row's drawing and the
  demonstration are the *same* drawing at two sizes: a thumbnail that simplifies
  itself is a picture of a different gesture from the one it stands in for. They
  all loop, each starting somewhere else in the shared cycle, so a list of them
  reads like a room with people in it rather than a warning light. Do not add a
  sample-data demo mode.
- **Offer one action per gesture, all visible.** A gesture binds one config
  field, which is the shape the config has, so no two rows can contend for one
  swipe. List every action the engine accepts for that field rather than hiding
  them in a menu: they are whole sentences carrying their own direction
  ("Mission Control up, app windows down"), and a menu shows one at a time and
  trims it.
- **Offer only gestures the surface can tell apart.** Multi-finger gestures are
  swipes; pinching and double-tapping are not offered, because on a phone-sized
  pad they are separated from a two-finger swipe by millimetres.
- **Start with common choices.** Put pointer speed, scrolling and other everyday
  controls in Basic settings. Keep gesture assignments in Gestures and timing
  thresholds in Advanced settings. Use disclosures for diagnostic readings and
  detailed computer preferences.
- **One page, one job.** The settings open on the devices connected to this
  computer, with the three groups of settings behind cards under them; each is a
  page reached from and returned to that home. Every card says what is currently
  set behind it, so the common question is answered without opening anything.
  Past a handful of settings, everything one tap away and all of it on screen at
  once is the same thing as nothing being findable.
- **Put a short answer on the page, not behind a card.** The device list is
  four lines that answer their own question by being read; a card saying "2
  connected" charges a tap for information that fits in the space the tap was
  going to use. Cards are for rooms, not for captions.
- **Move like something with mass.** Anything crossing the screen - a fingertip
  on a gesture drawing, a page arriving - accelerates, blurs while it is fast,
  and settles. The blur is what separates a thing that travelled from a thing
  that was swapped, and on a gesture drawing it is also what says which way the
  gesture went once it has stopped.
- **Explain state.** Distinguish connecting, connected, saving and saved.
  A computer-controlled setting needs an explanation of where its value comes
  from. Colour supports the words; it does not replace them.
- **Make touch comfortable.** Use at least 44 px navigation and button targets,
  readable system type, visible keyboard focus, and input text large enough to
  avoid unwanted mobile focus zoom.

## Visual language

The shared theme uses a dark navy background, pale periwinkle actions, rounded
panels and generous space between related groups. Sentence case and system
sans-serif type make labels and explanations readable together. Reserve
monospace or tabular numbers for actual diagnostic values.

| Token | Role |
|---|---|
| `--bg` · `#0c1018` | Page background |
| `--panel` · `#151c28` | Grouped settings and information |
| `--raised` · `#1d2736` | Secondary controls |
| `--line` · `#2b3748` | Quiet boundaries |
| `--fg` · `#edf2fa` | Main text |
| `--muted` · `#a7b4c8` | Supporting text |
| `--accent` · `#93a7ff` | Primary actions and selected state |

Rounded controls are intentional. The earlier restriction on rounded corners
has been replaced by this simpler, more familiar interface.

## Mobile layout and motion

The trackpad must fit the visible browser viewport on the first load and after
browser bars, orientation or the keyboard change. Its canvas and touch surface
must use the same rectangle. Settings must remain reachable within safe areas,
with an independently scrollable sheet and an obvious way to close it.

Document pages such as settings and troubleshooting scroll normally. They must
not inherit the trackpad’s fixed-height scrolling restrictions. Narrow screens
use one column, allow long labels to wrap, and retain usable controls without
horizontal page overflow.

Animate to explain a gesture or show a state change. Keep movement restrained,
respect reduced motion.
The drag-feedback canvas remains shared with the press preview so the preview
shows the same feedback as the live trackpad.

## Implementation

The interface remains TypeScript, and its CSS is Tailwind. Every layout,
spacing, colour and type decision is a utility class in the markup - in the five
pages, and in the TypeScript that generates the rest of it - so a rule can be
read off the element it applies to. `theme.css` is the only hand-written
stylesheet, and holds only what a utility class cannot be: the `@theme` tokens
the utilities are generated from, element defaults for the form controls whose
pseudo-elements no class can reach, and the handful of component classes whose
*name* is the interface, because TypeScript writes it or a check asserts it.

The one thing a class in the markup buys at a cost is that Tailwind only
generates rules for names it can find spelled out in full, so a class assembled
from a variable produces nothing and reports nothing. That decides how anything
generated is dressed - a component class for a state, a descendant variant on
the container for a place - and it is why a page is looked at in a browser
rather than signed off from a passing build. Both patterns, and the symptom of
getting it wrong, are in [gotchas](gotchas.md#the-interface).

The redesign does not need a framework or programming-language migration; the
existing touch capture, connection protocol and desktop gesture handling remain
reusable.

| File | Responsibility |
|---|---|
| `web/src/theme.css` | The Tailwind entry: tokens, breakpoints, control defaults, and the few component classes |
| `web/index.html` | Trackpad and quick settings |
| `web/config.html` | Full settings layout |
| `web/debug.html` | Connection diagnostics |
| `web/haptics.html` | Feedback checks |
| `web/preview.html` | Press preview layout |
| `web/src/config.ts` | Full settings: the hub, the four pages, and the form generated from the config the desktop sends |
| `web/src/devicespanel.ts` | Connected devices: the `/devices` channel, and un-pairing from the computer |
| `web/src/gestures.ts` | The gesture catalogue and action labels |
| `web/src/config.css` | The gesture drawings: an SVG animation engine, which is not styling and has no utilities |
| `web/src/style.css` | The trackpad frame's viewport-height fallback pair, which one utility cannot hold |
| `desktop/src/assets/connect.html` | The connect page: the QR, and the paired devices |

The connect page is served by the app rather than built by Vite, because only
the app can draw the QR and hand the page its key - so it carries its own inline
copy of the palette. Keep that copy's colours, controls and spacing in step when
changing the shared theme.

The pages are hash routes in one document - `#home`, `#basics`, `#gestures`,
`#advanced` - rather than files of their own, so there is one
socket, one header and one challenge, and `/config.html#gestures` from the
trackpad's toolbar still opens the gestures directly.

Basic settings is a single column with pointer speed, scrolling speed, natural
scrolling, and press-and-drag. Acceleration, zoom, timings, and other specialist
options live in collapsed Advanced groups. Gestures is three columns - the
gestures, the one selected demonstrated, the actions it can be given - placed by
grid column rather than reordered, so a gesture with nothing to assign leaves a
gap instead of moving the other two.

The drawings come from `web/src/gesturepad.ts`, built out of the `Gesture`
definition alone - fingers, motion, axis, direction and origin - so a gesture
added to `gestures.ts` is pictured without touching the drawing code. They are
SVG animated by CSS keyframes rather than a canvas driven by
`requestAnimationFrame`: there is no loop to stop when the gesture changes or
the tab is hidden, and `prefers-reduced-motion` is honoured by the stylesheet.

The motion blur is two gradient-filled capsules behind each fingertip rather
than a `filter`, grown and retracted by a dash offset on the travel's own clock
and easing. Twenty drawings with up to four fingers each is eighty filtered
layers to re-rasterise every frame, to soften an edge a gradient softens for
free.

Settings always come from the authenticated computer connection. There is no
sample-data demo mode. Settings behaviour is checked by
`web/scripts/check-settings.mjs` (`npm run check`).
