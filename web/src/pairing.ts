/**
 * Where is the desktop app? (plan.md section 7)
 *
 * The address, and the pairing secret that goes with it, come from the URL
 * fragment the QR encodes, from the last successful connection in localStorage,
 * or from a dev-time env var.
 *
 * The secret is in the **fragment** and nowhere else, and that is the whole
 * reason it is safe to put in a link at all: a fragment is never sent in an HTTP
 * request, so it reaches neither the server that served this page, nor its
 * access log, nor a proxy, nor a `Referer` header. Everything here keeps it on
 * that side of the `#`.
 */

import { answerChallenge } from "./hmac";

const STORE_KEY = "padremote.link.v1";
const CREDENTIAL_KEY = "padremote.credential.v1";

export interface Link {
  /** "host:port" of the desktop app. */
  host: string;
  /** Computer name, once the desktop has told us. */
  name?: string;
  /**
   * The pairing secret, hex, as it came off the QR.
   *
   * Without it the desktop's challenge cannot be answered and the connection is
   * closed before the first touch, which is exactly what should happen to a
   * page that was opened by hand rather than scanned.
   */
  key?: string;
  /**
   * Where the address came from.
   *
   * The difference that matters is `guess`: every other source means this phone
   * has been paired with a computer at some point, and failing to reach it is a
   * network or "is it running?" problem. A guess means nobody ever told this
   * page anything - the page was opened directly rather than from a QR - and
   * the advice for that is to scan the code, not to check the Wi-Fi.
   */
  source?: "fragment" | "stored" | "env" | "guess";
}

function readStored(): Link | null {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    return raw ? (JSON.parse(raw) as Link) : null;
  } catch {
    // Private browsing, or storage disabled. Not fatal: the fragment still works.
    return null;
  }
}

export function remember(link: Link): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(link));
  } catch {
    /* nothing to do; the page still works for this session */
  }
}

/**
 * The remembered address, without touching the URL.
 *
 * The settings page uses its fragment for the open tab, so it reads the saved
 * connection directly instead of interpreting that fragment as pairing data.
 */
export function storedLink(): Link | null {
  return readStored();
}

export function forget(): void {
  try {
    localStorage.removeItem(STORE_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Resolve the desktop address, most explicit source first.
 *
 * 1. `#h=host:port` - what the QR will carry
 * 2. localStorage    - reconnect without a scan
 * 3. VITE_DESKTOP_WS - developer override
 * 4. this page's own host - the common case when the desktop also serves the page
 */
export function resolveLink(): Link {
  const frag = new URLSearchParams(location.hash.replace(/^#/, ""));
  const fromHash = frag.get("h");
  if (fromHash) {
    const link: Link = {
      host: fromHash,
      name: frag.get("n") ?? undefined,
      key: frag.get("k") ?? undefined,
      source: "fragment",
    };
    remember(link);
    // Keep navigation untouched while Chrome opens from the QR scanner.
    // Retaining the fragment also preserves pairing on reload if storage is
    // unavailable. It contains the host/name, not an element to scroll to.
    return link;
  }

  const stored = readStored();
  if (stored?.host) return { ...stored, source: "stored" };

  const env = import.meta.env.VITE_DESKTOP_WS as string | undefined;
  if (env) {
    return {
      host: env.replace(/^wss?:\/\//, ""),
      key: import.meta.env.VITE_DESKTOP_KEY as string | undefined,
      source: "env",
    };
  }

  return { host: guessHost(), source: "guess" };
}

/**
 * Where the desktop probably is, when nothing has said.
 *
 * The shipped app serves this page and accepts the socket on the *same* port,
 * so its own address is the answer - which is what makes typing the address
 * into a phone by hand work, with no QR and no fragment.
 *
 * Vite is the exception: in development the page comes from 5173 and the
 * desktop is on its own port, so guessing "wherever this page came from" would
 * point the socket back at the dev server.
 */
function guessHost(): string {
  const DEV_SERVER_PORT = "5173";
  if (location.port && location.port !== DEV_SERVER_PORT) return location.host;
  return `${location.hostname}:8787`;
}

/**
 * Take the pairing secret out of the fragment, remember it, and remove it from
 * the address bar.
 *
 * For the pages the *computer* opens - settings, diagnostics - rather than the
 * phone page, which keeps its fragment so a reload still works when storage is
 * unavailable. Here it is worth stripping twice over: those pages use the
 * fragment for their own routing, and a secret left in a desktop address bar
 * ends up in a screenshot or a shared screen sooner or later.
 */
export function takeKeyFromFragment(): string | undefined {
  const frag = new URLSearchParams(location.hash.replace(/^#/, ""));
  const key = frag.get("k") ?? undefined;
  if (key) {
    const stored = readStored();
    // Merge rather than replace: this page knows the key but usually not the
    // host, and overwriting the remembered link with a hostless one would
    // un-pair the phone page in the same browser.
    remember({ ...(stored ?? { host: "" }), key });
    frag.delete("k");
    const rest = frag.toString();
    try {
      history.replaceState(null, "", `${location.pathname}${location.search}${rest ? `#${rest}` : ""}`);
    } catch {
      /* a browser that refuses the rewrite still has the key; carry on */
    }
  }
  return key ?? readStored()?.key ?? undefined;
}

/**
 * Build the socket URL.
 *
 * A page served over HTTPS may only open `wss://`. Milestone 2 therefore serves
 * the page over plain http on the LAN; milestone 3 brings the self-signed cert
 * that lets both sides be secure (plan.md section 15).
 */
export function socketUrl(link: Link): string {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${link.host}`;
}

/**
 * This device's own credential, once it has enrolled.
 *
 * Kept apart from the link above because the two have different lifetimes and
 * different consequences. The link is an address and can be re-guessed; this is
 * what proves to the computer that this device is one it agreed to, and it is
 * the *only* secret the phone keeps once enrolment is done.
 */
export interface Credential {
  /** 32 hex characters, chosen here, public. */
  id: string;
  /** Derived from the QR secret; never sent anywhere. */
  key: string;
}

export function storedCredential(): Credential | null {
  try {
    const raw = localStorage.getItem(CREDENTIAL_KEY);
    if (!raw) return null;
    const cred = JSON.parse(raw) as Credential;
    return cred.id && cred.key ? cred : null;
  } catch {
    return null;
  }
}

/**
 * Remember this device's credential. False when storage refused it.
 *
 * The caller has to know: the QR secret is only safe to throw away once this
 * has actually stuck, and in private browsing it does not.
 */
export function rememberCredential(cred: Credential): boolean {
  try {
    localStorage.setItem(CREDENTIAL_KEY, JSON.stringify(cred));
    return storedCredential() !== null;
  } catch {
    return false;
  }
}

export function forgetCredential(): void {
  try {
    localStorage.removeItem(CREDENTIAL_KEY);
  } catch {
    /* nothing to do; the next scan enrols afresh anyway */
  }
}

/**
 * Throw away the QR's enrolment secret, everywhere it lives.
 *
 * This is what makes revoking a device mean anything. While the phone still
 * holds the secret, "forget this device" is undone by it enrolling itself again
 * under a new id; once it holds only its own key, being forgotten is final and
 * the way back is to be shown the code again.
 *
 * It also takes the secret out of the address bar, where it was one screenshot
 * or one shared screen away from being handed to somebody.
 */
export function discardEnrolmentSecret(): void {
  const stored = readStored();
  if (stored?.key) {
    const { key: _dropped, ...rest } = stored;
    remember(rest);
  }
  const frag = new URLSearchParams(location.hash.replace(/^#/, ""));
  if (!frag.has("k")) return;
  frag.delete("k");
  const rest = frag.toString();
  try {
    history.replaceState(
      null,
      "",
      `${location.pathname}${location.search}${rest ? `#${rest}` : ""}`,
    );
  } catch {
    /* a browser that refuses the rewrite keeps the secret in view; nothing
       else here depends on the URL, and the stored copy is gone either way */
  }
}

/**
 * How to answer the desktop's challenge, for a page that never enrols.
 *
 * The settings and diagnostics pages, which are opened either from the phone -
 * where this device already has a credential of its own and the QR secret is
 * long gone - or from the computer's own menu bar, where the link carries the
 * secret and there is no credential to have. Both cases, in that order, because
 * a device that has enrolled must sign as itself: it is the only key it still
 * has, and the only one that can be revoked.
 *
 * Enrolment deliberately does *not* happen here. Only the trackpad page mints a
 * credential, so opening a settings page can never quietly pair a device.
 */
export function authReply(
  nonce: string,
  enrolmentKey?: string,
): { hmac: string; device?: string } | null {
  const cred = storedCredential();
  if (cred) {
    const hmac = answerChallenge(cred.key, nonce);
    if (hmac) return { hmac, device: cred.id };
  }
  if (!enrolmentKey) return null;
  const hmac = answerChallenge(enrolmentKey, nonce);
  return hmac ? { hmac } : null;
}
