/** Independent gesture triggers and action labels. */
/** Which system the desktop is running, as it reports itself. */
export type Os = "macos" | "windows" | "linux";

/** How the fingers move. The pad drawing knows nothing else about a gesture. */
export type Motion = "tap" | "swipe" | "scroll" | "move" | "hold";

/** One gesture: a hand shape, and the config field it is bound to. */
export interface Gesture {
  id: string;
  /** Per-OS where the wording differs; `macos` doubles as the fallback. */
  name: Partial<Record<Os, string>> & { macos: string };
  fingers: number;
  motion: Motion;
  axis?: "x" | "y";
  direction?: "left" | "right" | "up" | "down";
  legacyField?: string;
  origin?: { x: number; y: number };
  /** The config field this gesture assigns, when it is assignable. */
  field?: string;
  /** What it does, for the gestures that are not configurable. */
  fixed?: string;
}

/** Pick the wording for this computer, falling back to the macOS phrasing. */
export function forOs(
  text: Partial<Record<Os, string>> & { macos: string },
  os: Os,
): string {
  return text[os] ?? text.macos;
}

export const GESTURES: Gesture[] = [
  {
    id: "move",
    name: { macos: "Move one finger" },
    fingers: 1,
    motion: "move",
    fixed: "Moves the cursor.",
  },
  {
    id: "pressAndDrag",
    name: { macos: "Press and hold, then move" },
    fingers: 1,
    motion: "hold",
    fixed:
      "Holds the button down, for dragging and selecting text. Your phone has no button, so this stands in for one.",
  },
  ...(["up", "down"] as const).map((direction): Gesture => ({
    id: `scroll${direction}`, name: { macos: `Swipe ${direction} with two fingers` },
    fingers: 2, motion: "scroll", axis: "y", direction,
    fixed: "Scrolls the window. Adjust scrolling in Basics.",
  })),
  {
    id: "oneTap",
    name: { macos: "Tap with one finger" },
    fingers: 1,
    motion: "tap",
    field: "bindings.oneTap",
  },
  {
    id: "twoFingerTap",
    name: { macos: "Tap with two fingers" },
    fingers: 2,
    motion: "tap",
    field: "bindings.twoFingerTap",
  },
  {
    id: "threeFingerTap",
    name: { macos: "Tap with three fingers" },
    fingers: 3,
    motion: "tap",
    field: "bindings.threeFingerTap",
  },
  {
    id: "fourFingerTap",
    name: { macos: "Tap with four fingers" },
    fingers: 4,
    motion: "tap",
    field: "bindings.fourFingerTap",
  },
  ...([2, 3, 4] as const).flatMap((fingers) => {
    const count = { 2: "two", 3: "three", 4: "four" }[fingers];
    const directions = fingers === 2 ? ["left", "right"] as const : ["left", "right", "up", "down"] as const;
    return directions.map((direction): Gesture => ({
      id: `${count}FingerSwipe${direction[0].toUpperCase()}${direction.slice(1)}`,
      name: { macos: `Swipe ${direction} with ${count} fingers` },
      fingers, motion: "swipe", direction,
      axis: direction === "left" || direction === "right" ? "x" : "y",
      field: `bindings.${count}FingerSwipe${direction[0].toUpperCase()}${direction.slice(1)}`,
      legacyField: `bindings.${fingers === 2 ? "twoFingerSwipeNavigate" : `${count}Finger${direction === "left" || direction === "right" ? "Horiz" : "Vert"}Swipe`}`,
    }));
  }),
];

export function gestureSections(): { title: string; gestures: Gesture[] }[] {
  return [
    { title: "Pointer & taps", gestures: GESTURES.filter((g) => !g.direction) },
    ...[2, 3, 4].map((fingers) => ({
      title: `${fingers}-finger swipes`,
      gestures: GESTURES.filter((g) => g.direction && g.fingers === fingers),
    })),
  ];
}

/** Every field the gesture list assigns, so the rest falls through elsewhere. */
export function claimedFields(): Set<string> {
  return new Set(GESTURES.flatMap((g) => [g.field, g.legacyField]).filter((f): f is string => f !== undefined));
}

/**
 * What an action does, named with the directions it applies to.
 *
 * "Mission Control" alone is half the story on a swipe that binds a whole axis:
 * up opens it and down shows the app's windows. The direction belongs in the
 * name, where the choice is being made, rather than in a footnote under it.
 */
const ACTION_NAMES: Record<string, Partial<Record<Os, string>> & { macos: string }> = {
  inherit: { macos: "Use existing setting" },
  desktopLeft: { macos: "Desktop left", linux: "Workspace left" },
  desktopRight: { macos: "Desktop right", linux: "Workspace right" },
  back: { macos: "Back" }, forward: { macos: "Forward" },
  volumeUp: { macos: "Volume up" }, volumeDown: { macos: "Volume down" },
  brightnessUp: { macos: "Brightness up" }, brightnessDown: { macos: "Brightness down" },
  zoomIn: { macos: "Zoom in" }, zoomOut: { macos: "Zoom out" },
  previousTab: { macos: "Previous tab" }, nextTab: { macos: "Next tab" },
  undo: { macos: "Undo" }, redo: { macos: "Redo" },
  none: { macos: "Do nothing" },
  leftClick: { macos: "Left click" },
  rightClick: { macos: "Right click" },
  middleClick: { macos: "Middle click" },
  // Only macOS has a smart zoom; elsewhere the nearest thing a double tap can
  // mean is resetting the zoom, and that is what the injector sends.
  smartZoom: { macos: "Smart zoom", windows: "Reset zoom", linux: "Reset zoom" },
  navigate: { macos: "Back and forward" },
  spaces: {
    macos: "Switch full-screen apps, left and right",
    windows: "Switch virtual desktops, left and right",
    linux: "Switch workspaces, left and right",
  },
  missionControl: {
    macos: "Mission Control up, app windows down",
    windows: "Task View up, show the desktop down",
    linux: "Overview up, show the desktop down",
  },
  // "App Exposé" is what System Settings calls it, accent and all. The label
  // here says what it does instead: the accented word is hard to type into
  // the search box above the list, and hard to read for anyone whose English
  // stops short of Apple's branding.
  appWindows: { macos: "App windows", windows: "Task View", linux: "Window overview" },
  volume: { macos: "Volume up and down" },
  brightness: { macos: "Brightness up and down" },
  zoom: { macos: "Zoom in and out" },
  tabs: { macos: "Previous and next tab" },
  undoRedo: { macos: "Undo and redo" },
  launchpad: { macos: "Launchpad", windows: "Start menu", linux: "Applications" },
  showDesktop: { macos: "Show Desktop" },
  switchApps: { macos: "Switch apps" },
  spotlight: { macos: "Spotlight", windows: "Search" },
  screenshot: { macos: "Screenshot" },
  lockScreen: { macos: "Lock screen" },
  mute: { macos: "Mute and unmute" },
  copy: { macos: "Copy" },
  cut: { macos: "Cut" },
  paste: { macos: "Paste" },
  selectAll: { macos: "Select all" },
  save: { macos: "Save" },
  find: { macos: "Find" },
  newTab: { macos: "New tab" },
  closeWindow: { macos: "Close window" },
  minimiseWindow: { macos: "Minimise window" },
  quitApp: { macos: "Quit app" },
  fullScreen: { macos: "Full screen" },
  calculator: { macos: "Calculator" },
};

/**
 * The handful worth suggesting, per kind of gesture.
 *
 * A tap can now be bound to twenty-odd things. Sorted flat that is a wall to
 * read every time, and alphabetical order puts "Calculator" above "Mission
 * Control" - true, and useless. So the few anyone actually wants come first,
 * and the rest are listed alphabetically underneath for whoever is looking for
 * something specific.
 */
const RECOMMENDED_SINGLE = [
  "inherit", "desktopLeft", "desktopRight", "appWindows",
  "leftClick",
  "rightClick",
  "middleClick",
  "missionControl",
  "showDesktop",
  "spotlight",
  "screenshot",
];
const RECOMMENDED_PAIRED = ["spaces", "missionControl", "volume", "brightness", "zoom", "navigate"];

/**
 * The actions available for one gesture: the recommended few, then the rest.
 *
 * Driven by the vocabulary the desktop sends, so an action the engine gains
 * appears here without anything being added, and one it drops disappears.
 */
export function groupActions(
  options: string[],
  os: Os,
  paired: boolean,
): { title: string; actions: string[] }[] {
  const groups: { title: string; actions: string[] }[] = [];
  const seen = new Set<string>();
  // "Nothing" is not a category; it belongs at the top, ungrouped.
  if (options.includes("none")) {
    groups.push({ title: "", actions: ["none"] });
    seen.add("none");
  }
  const recommended = (paired ? RECOMMENDED_PAIRED : RECOMMENDED_SINGLE).filter((a) =>
    options.includes(a),
  );
  recommended.forEach((a) => seen.add(a));
  if (recommended.length) groups.push({ title: "Recommended", actions: recommended });

  const rest = options
    .filter((a) => !seen.has(a))
    .sort((a, b) => actionName(a, os, paired).localeCompare(actionName(b, os, paired)));
  if (rest.length) groups.push({ title: "Other actions", actions: rest });
  return groups;
}

/**
 * The same action, named without its directions.
 *
 * A swipe binds a pair and the name says so - "Mission Control up, app windows
 * down". A tap has no up or down, and reading that on a tap describes a gesture
 * the user is not making.
 */
const SINGLE_NAMES: Record<string, Partial<Record<Os, string>> & { macos: string }> = {
  missionControl: { macos: "Mission Control", windows: "Task View", linux: "Overview" },
};

/**
 * The action's name for this computer, or the raw value if it is unknown.
 *
 * `paired` is whether the gesture performing it has two directions.
 */
export function actionName(value: string, os: Os, paired = true): string {
  const name = (!paired && SINGLE_NAMES[value]) || ACTION_NAMES[value];
  return name ? forOs(name, os) : `${value} (unknown)`;
}

/** Does this gesture have two directions for an action to split across? */
export function isPaired(g: Gesture): boolean {
  return !g.direction && (g.motion === "swipe" || g.motion === "scroll");
}

/** Kept for the fields Advanced still renders as a plain menu. */
export const ACTIONS: Record<string, string> = Object.fromEntries(
  Object.entries(ACTION_NAMES).map(([k, v]) => [k, v.macos]),
);

/** Resolve old paired bindings without changing a user's existing setup. */
export function inheritedAction(g: Gesture, legacy: string, natural: boolean): string {
  const positive = g.direction === "right" || g.direction === "down";
  const back = natural ? positive : !positive;
  switch (legacy) {
    case "spaces": return back ? "desktopLeft" : "desktopRight";
    case "navigate": return back ? "back" : "forward";
    case "missionControl": return positive ? "appWindows" : "missionControl";
    case "volume": return positive ? "volumeDown" : "volumeUp";
    case "brightness": return positive ? "brightnessDown" : "brightnessUp";
    case "zoom": return positive ? "zoomOut" : "zoomIn";
    case "tabs": return positive ? "nextTab" : "previousTab";
    case "undoRedo": return positive ? "redo" : "undo";
    default: return legacy;
  }
}
