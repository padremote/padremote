/**
 * Haptic feedback, as far as the web platform allows.
 *
 * A long press has to *announce itself*, or the user cannot tell a drag from a
 * move until something has already been selected. On a real trackpad the click
 * is felt; the phone has to substitute for that.
 *
 * Support is uneven and worth stating plainly:
 *
 *   - **Android Chrome** implements `navigator.vibrate`, but gates it behind the
 *     page having been *activated* by a real tap, and it stays silent - no
 *     exception, no console warning worth the name - when it has not been. See
 *     `suppressBrowserGestures` in `surface.ts` for the way a page can lose that
 *     activation without ever knowing.
 *   - **iOS Safari does not implement it at all** - `navigator.vibrate` is
 *     simply undefined, and no amount of user gesture will conjure it.
 *
 * For iOS there is one documented trick: since 17.4, toggling a switch-styled
 * checkbox (`<input type="checkbox" switch>`) plays a light system haptic. It is
 * a side effect rather than an API, so it is used strictly as a bonus - the
 * visual feedback is what every device actually relies on.
 */

/**
 * A single crisp tap, for "the drag has started".
 *
 * Kept well clear of the 10-15 ms mark: the spec takes any duration, but real
 * vibration motors need a few tens of milliseconds to spin up, and Samsung's
 * One UI in particular renders a sub-20 ms request as nothing at all. 30 ms is
 * the shortest pulse that is reliably *felt* rather than merely requested.
 */
export const TAP_PATTERN = 30;

let iosSwitch: HTMLInputElement | null = null;

/** True when the browser has a real vibration API. */
export function hasVibration(): boolean {
  return typeof navigator.vibrate === "function";
}

/**
 * Build the hidden switch iOS needs. Safe to call anywhere; on every other
 * browser it costs one unused element.
 */
function ensureIosSwitch(): HTMLInputElement {
  if (iosSwitch) return iosSwitch;
  const el = document.createElement("input");
  el.type = "checkbox";
  // The `switch` attribute is what makes iOS treat it as a haptic control.
  el.setAttribute("switch", "");
  el.setAttribute("aria-hidden", "true");
  el.tabIndex = -1;
  // Off-screen rather than display:none — a hidden control emits nothing.
  el.style.cssText =
    "position:fixed;left:-9999px;top:0;width:1px;height:1px;opacity:0;pointer-events:none";
  document.body.appendChild(el);
  iosSwitch = el;
  return el;
}

/**
 * Buzz, if this device can.
 *
 * Never throws and never blocks: haptics are a nicety, and a browser that
 * refuses must not take the gesture down with it.
 */
export function buzz(pattern: number | number[] = TAP_PATTERN): boolean {
  try {
    if (typeof navigator.vibrate === "function") {
      // Chrome returns false when it refuses the call - most often because the
      // page holds no user activation. Worth reporting; never worth throwing.
      return navigator.vibrate(pattern);
    }
    // iOS: toggling a switch plays a light haptic as a side effect.
    const el = ensureIosSwitch();
    el.checked = !el.checked;
    el.dispatchEvent(new Event("change", { bubbles: false }));
    return false;
  } catch {
    /* haptics are optional; never let them break a gesture */
    return false;
  }
}

/**
 * What this device can actually do, for the debug page.
 *
 * `activated` is the interesting field: it is the difference between "this
 * browser has no vibration motor" and "this browser has one and is refusing to
 * use it", which are otherwise indistinguishable from a silent phone.
 */
export function hapticsReport(probe = true): {
  api: boolean;
  activated: boolean | null;
  accepted: boolean | null;
} {
  const nav = navigator as Navigator & { userActivation?: { hasBeenActive: boolean } };
  return {
    api: hasVibration(),
    activated: nav.userActivation ? nav.userActivation.hasBeenActive : null,
    // Reporting must not buzz of its own accord - the first render happens on
    // page load, where a stray pulse would be both rude and uninformative.
    accepted: probe ? buzz() : null,
  };
}
