/**
 * Giving the pad the whole screen, and taking it back.
 *
 * What "full screen" can mean depends entirely on where the page is running,
 * and it is worth being blunt about the worst case:
 *
 * - **Android Chrome, iPad, desktop**: the Fullscreen API works. The page fills
 *   the display and the browser's chrome goes away.
 * - **iPhone Safari**: there is *no* Fullscreen API for an element. Apple has
 *   never shipped it outside `<video>`, and no amount of asking changes that.
 *   The best available is to hide the pad's own chrome and scroll the page a
 *   pixel, which is what persuades Safari to collapse its address bar into the
 *   compact one and drop the toolbar.
 * - **Installed to the Home Screen**: the manifest asks for `display:
 *   standalone`, so the browser chrome is not there to begin with. On an iPhone
 *   that is the only route to a genuinely full screen, which is why the page
 *   says so once rather than pretending otherwise.
 *
 * All three are the same mode as far as the rest of the app is concerned:
 * `document.body` carries `.immersive`, and everything that should get out of
 * the way keys off that.
 */

export type Immersive = "off" | "fullscreen" | "chrome-hidden";

/** Already running without browser chrome, because it was installed. */
export function isStandalone(): boolean {
  return (
    matchMedia("(display-mode: standalone)").matches ||
    // iOS predates the display-mode media query for this.
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  );
}

/** Does this browser have the real thing? */
export function hasFullscreen(): boolean {
  const el = document.documentElement as HTMLElement & {
    webkitRequestFullscreen?: () => Promise<void>;
  };
  return typeof el.requestFullscreen === "function" || typeof el.webkitRequestFullscreen === "function";
}

function fullscreenElement(): Element | null {
  const d = document as Document & { webkitFullscreenElement?: Element | null };
  return document.fullscreenElement ?? d.webkitFullscreenElement ?? null;
}

/**
 * Nudge the page down a pixel.
 *
 * The one lever a page has over Safari's chrome: a scrollable document that has
 * been scrolled is a document Safari collapses its bars for. The pad has
 * `overflow: hidden`, so a single pixel of scrollable height is added while
 * immersive and taken away after - any more and the surface could be dragged
 * around under the finger.
 */
function nudgeScroll(on: boolean): void {
  document.body.classList.toggle("nudge", on);
  if (on) {
    // After the class has been applied, or there is nothing to scroll yet.
    requestAnimationFrame(() => window.scrollTo(0, 1));
  } else {
    window.scrollTo(0, 0);
  }
}

let state: Immersive = "off";

export function immersiveState(): Immersive {
  return state;
}

/**
 * Toggle it. Returns the state afterwards, so the caller can say what happened
 * rather than guess.
 */
export async function toggleImmersive(): Promise<Immersive> {
  if (state !== "off") {
    if (fullscreenElement()) {
      const d = document as Document & { webkitExitFullscreen?: () => Promise<void> };
      try {
        await (document.exitFullscreen?.() ?? d.webkitExitFullscreen?.());
      } catch {
        /* already gone, or refused - the class comes off either way */
      }
    }
    nudgeScroll(false);
    document.body.classList.remove("immersive");
    state = "off";
    return state;
  }

  document.body.classList.add("immersive");
  const el = document.documentElement as HTMLElement & {
    webkitRequestFullscreen?: () => Promise<void>;
  };
  try {
    if (typeof el.requestFullscreen === "function") {
      await el.requestFullscreen({ navigationUI: "hide" });
      state = "fullscreen";
      return state;
    }
    if (typeof el.webkitRequestFullscreen === "function") {
      await el.webkitRequestFullscreen();
      state = "fullscreen";
      return state;
    }
  } catch {
    // Refused - Safari does this outside a user gesture, and a shake is not
    // one. Fall through to the version that needs no permission.
  }
  nudgeScroll(true);
  state = "chrome-hidden";
  return state;
}

/**
 * Keep our idea of the state honest when the *browser* leaves fullscreen -
 * Escape on a desktop, the swipe-down on Android. Without this the next shake
 * would try to exit a fullscreen that had already ended.
 */
export function watchFullscreenExit(onExit: () => void): void {
  const check = () => {
    if (state === "fullscreen" && !fullscreenElement()) {
      nudgeScroll(false);
      document.body.classList.remove("immersive");
      state = "off";
      onExit();
    }
  };
  document.addEventListener("fullscreenchange", check);
  document.addEventListener("webkitfullscreenchange", check);
}
