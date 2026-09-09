/**
 * What this device calls itself.
 *
 * Several phones can drive one computer at the same time, taking turns with the
 * cursor. The moment there are two, "another device is in control" is useless on
 * its own - the user has to know *which* one, and "192.168.1.42" is not an
 * answer anybody recognises. So the page names itself, guesses well enough to be
 * right most of the time, and lets the user correct it in Settings.
 */

const KEY = "padremote.device.v1";

/**
 * A guess from the user agent.
 *
 * Deliberately coarse. The point is to tell *this* phone apart from the tablet
 * on the sofa, not to identify a model - and a wrong-but-plausible name is worse
 * than a vague one, so nothing here claims more than the UA actually says.
 */
function guess(): string {
  const ua = navigator.userAgent;
  // iPadOS reports itself as a Mac; the touch points are what give it away.
  const iPadInDisguise = /Macintosh/.test(ua) && navigator.maxTouchPoints > 1;
  if (/iPad/.test(ua) || iPadInDisguise) return "iPad";
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPod/.test(ua)) return "iPod";
  if (/Android/.test(ua)) {
    // Android's own convention: a tablet omits "Mobile" from the token.
    const kind = /Mobile/.test(ua) ? "phone" : "tablet";
    if (/SM-|SAMSUNG|Galaxy/i.test(ua)) return `Samsung ${kind}`;
    if (/Pixel/.test(ua)) return `Pixel ${kind}`;
    return `Android ${kind}`;
  }
  if (/Windows/.test(ua)) return "Windows PC";
  if (/Macintosh/.test(ua)) return "Mac";
  if (/Linux/.test(ua)) return "Linux";
  return "This device";
}

/** Trimmed to what the desktop will accept, so what is shown is what is sent. */
export function clean(name: string): string {
  return name.trim().replace(/\s+/g, " ").slice(0, 32);
}

export function deviceName(): string {
  try {
    const stored = clean(localStorage.getItem(KEY) ?? "");
    if (stored) return stored;
  } catch {
    /* storage unavailable; the guess is still fine */
  }
  return guess();
}

export function setDeviceName(name: string): void {
  const value = clean(name);
  try {
    if (value) localStorage.setItem(KEY, value);
    else localStorage.removeItem(KEY);
  } catch {
    /* ignore: the name still applies for this session */
  }
}
