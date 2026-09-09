/**
 * WebSocket link to the desktop app (plan.md sections 7 and 8).
 *
 * One socket carries both logical streams: binary touch batches and JSON
 * control messages. It reconnects on its own with 1 s -> 10 s backoff, because
 * a phone that has to be told to reconnect is a phone the user stops trusting.
 */

import { encodeFrame, Phase, type ServerMessage, type TouchSample } from "./protocol";
import {
  discardEnrolmentSecret,
  forgetCredential,
  rememberCredential,
  socketUrl,
  storedCredential,
  type Credential,
  type Link,
} from "./pairing";
import { deviceName } from "./device";
import { answerChallenge, deriveDeviceKey, newDeviceId } from "./hmac";

/**
 * `superseded` survives for older desktops only.
 *
 * The desktop now lets several devices stay connected and take turns with the
 * cursor, so nothing is ever evicted; a build from before that still hangs up on
 * the loser, and this page must not fight it. Kept apart from `offline` because
 * the two need opposite handling - one retries, the other must not.
 */
export type Status =
  | "connecting"
  | "connected"
  | "offline"
  | "superseded"
  | "unpaired"
  /** A newer page on this same device took over - another tab, or another browser. */
  | "replaced";

export interface LinkEvents {
  onStatus: (status: Status) => void;
  onMessage: (msg: ServerMessage) => void;
}

const BACKOFF_MIN_MS = 1000;
const BACKOFF_MAX_MS = 10_000;

/**
 * How much unsent touch data may pile up in the socket before moves are thinned.
 *
 * A touch frame is `2 + 14 * samples` bytes, so this is a few frames' worth -
 * enough to ride out one slow moment, small enough that it cannot become a
 * queue the cursor is playing back from seconds later.
 *
 * The pile-up is real and it is what two connected devices feel like: the page
 * hands the socket a frame every animation frame whatever happens, so if the
 * link cannot drain at that rate, `send` keeps accepting and the delay between
 * a finger and a cursor grows without bound and without any signal. Dropping
 * stale intermediate positions is the right answer, because that is what they
 * are - the newest position is the one the user's finger is actually at.
 */
const MAX_BUFFERED_BYTES = 512;

export class DesktopLink {
  private ws: WebSocket | null = null;
  /**
   * Has this socket answered the desktop's challenge?
   *
   * An open socket is not yet a usable one. The desktop refuses every frame
   * that arrives before the answer - including the first touch batch - so the
   * page must not call itself connected, or send anything, until this is true.
   */
  private authed = false;
  /**
   * A credential derived during this connection's handshake, not yet kept.
   *
   * Enrolment is only real once the desktop has accepted it, and the desktop
   * says so by carrying on rather than by answering. Storing it early - and
   * throwing the QR secret away with it - would strand the phone on the one
   * path where the answer turns out to be wrong: it would hold a key the
   * computer does not know and no way left to enrol again.
   */
  private pending: Credential | null = null;
  private backoff = BACKOFF_MIN_MS;
  private retryTimer: number | null = null;
  private closed = false;
  /** Geometry to announce on connect; set before `start`. */
  geometry: { wpx: number; hpx: number; dpr: number } = { wpx: 0, hpx: 0, dpr: 1 };

  constructor(
    private link: Link,
    private readonly events: LinkEvents,
  ) {}

  get url(): string {
    return socketUrl(this.link);
  }

  start(): void {
    this.closed = false;
    // A page being torn down must take its socket with it, or the desktop is
    // left holding a connection that the next load then has to evict.
    window.addEventListener("pagehide", () => this.stop());
    this.connect();
  }

  stop(): void {
    this.closed = true;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.ws?.close();
    this.ws = null;
  }

  /**
   * Give up the link at an older desktop's request, and stop reconnecting.
   *
   * A desktop from before multi-device support hands the cursor to the newest
   * connection and hangs up on the rest. A page that reconnects after being
   * evicted evicts the phone that took over, whose own reconnect evicts this one
   * right back - a tug of war, once per backoff, that leaves both trackpads
   * useless. Standing down ends it; the user takes control back with `resume`.
   * Against a current desktop this is never reached: nobody is evicted, and the
   * devices take turns instead.
   */
  standDown(): void {
    this.giveUp("superseded");
  }

  /**
   * Stop, and say why - without the reconnect that every other close triggers.
   *
   * The two reasons differ in what the user can do about them, so they are
   * different statuses, but the mechanics are identical: retrying an eviction
   * or a failed challenge produces the same failure once per backoff, forever.
   */
  giveUp(status: Status): void {
    this.closed = true;
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    this.authed = false;
    // Null it first: `onclose` ignores a socket that is no longer the live one,
    // which is what keeps this from being overwritten by an "offline" status.
    const ws = this.ws;
    this.ws = null;
    ws?.close();
    this.events.onStatus(status);
  }

  /** Take control back, at the user's request. */
  resume(): void {
    if (!this.closed) return;
    this.closed = false;
    this.backoff = BACKOFF_MIN_MS;
    this.connect();
  }

  /** Point at a different desktop, e.g. after scanning a new QR. */
  retarget(link: Link): void {
    this.link = link;
    this.backoff = BACKOFF_MIN_MS;
    // A new QR is also the way back from `unpaired`, so retargeting has to
    // undo the stand-down that being un-paired put this link into.
    this.closed = false;
    this.ws?.close();
    if (this.ws === null) this.connect();
  }

  private connect(): void {
    if (this.closed) return;
    // Never run two sockets at once. A second socket from this same page is a
    // second device as far as the desktop is concerned - it would take its turn
    // with the cursor, and its close would trigger another reconnect: a storm
    // that reads as the status flickering several times a second.
    const state = this.ws?.readyState;
    if (state === WebSocket.CONNECTING || state === WebSocket.OPEN) return;

    this.events.onStatus("connecting");

    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch {
      // A malformed address should retry, not throw into the console forever.
      this.scheduleRetry();
      return;
    }
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    this.authed = false;

    // Nothing is sent on open any more. The desktop speaks first, with a
    // challenge, and until that is answered this socket may as well be closed.
    ws.onopen = () => {
      this.backoff = BACKOFF_MIN_MS;
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      let msg: ServerMessage;
      try {
        msg = JSON.parse(ev.data) as ServerMessage;
      } catch {
        /* a control message we cannot read is not worth dropping the link for */
        return;
      }
      if (msg.t === "challenge") {
        this.answer(ws, msg.nonce);
        return;
      }
      // The desktop said something other than "no", so the handshake stuck.
      // This is the moment enrolment becomes real - and the moment the QR
      // secret stops being needed by anything.
      if (this.pending && msg.t !== "error") {
        if (rememberCredential(this.pending)) discardEnrolmentSecret();
        this.pending = null;
      }
      this.events.onMessage(msg);
    };
    ws.onerror = () => ws.close();
    ws.onclose = () => {
      // Ignore a socket we have already replaced: letting a stale close null out
      // the live socket is what turned one dropped connection into an endless
      // reconnect loop. `authed` belongs *below* this guard for the same
      // reason - `retarget` closes the old socket and opens a new one without
      // waiting, so the old close can land after the new one has authenticated,
      // and clearing the flag there would silently stop every touch on a link
      // whose status light still says connected.
      if (this.ws !== ws) return;
      this.authed = false;
      this.ws = null;
      this.events.onStatus("offline");
      this.scheduleRetry();
    };
  }

  /**
   * Prove we were paired with this computer, then start the session.
   *
   * A page with no secret - opened by hand, or from a stale bookmark - has
   * nothing to send. Saying so immediately is better than sending a wrong
   * answer and waiting to be hung up on: the advice the user needs is "scan the
   * QR", and it should not take a timeout to reach them.
   */
  private answer(ws: WebSocket, nonce: string): void {
    // A challenge arriving on a socket we have already replaced belongs to the
    // old link. Answering it would authenticate a connection nobody is using,
    // and giving up on it would tear down the one that is.
    if (this.ws !== ws) return;

    const reply = this.credentials(nonce);
    if (!reply) {
      this.giveUp("unpaired");
      return;
    }
    ws.send(JSON.stringify({ t: "auth", ...reply }));
    this.authed = true;
    // Only now is this a link: the welcome, the status light and everything the
    // page does on connect all hang off this point rather than off `onopen`.
    this.events.onStatus("connected");
    this.sendWelcome();
  }

  /**
   * Which key to answer with, and what to call ourselves while doing it.
   *
   * Two cases, and the desktop tells them apart by whether it already knows the
   * id (see `auth::devices`):
   *
   * - **enrolled already** - sign with this device's own key. The QR secret is
   *   long gone, and that is what lets the computer revoke this one device.
   * - **first time here** - mint an id, derive the key it implies, and sign
   *   with the QR secret to prove we were shown the code. The credential is
   *   held in `pending` until the desktop accepts it.
   */
  private credentials(nonce: string): { hmac: string; device: string } | null {
    const existing = storedCredential();
    if (existing) {
      const hmac = answerChallenge(existing.key, nonce);
      return hmac ? { hmac, device: existing.id } : null;
    }
    if (!this.link.key) return null;
    const hmac = answerChallenge(this.link.key, nonce);
    const id = newDeviceId();
    const key = deriveDeviceKey(this.link.key, id);
    if (!hmac || !key) return null;
    this.pending = { id, key };
    return { hmac, device: id };
  }

  /**
   * Forget this device's credential, so the next scan enrols cleanly.
   *
   * Called when the desktop refuses it - which, now that credentials can be
   * revoked one at a time, usually means exactly that: somebody chose *Forget
   * a device* on the computer. Keeping a key that is known not to work would
   * only make the next scan enrol under a second id.
   */
  forgetPairing(): void {
    this.pending = null;
    forgetCredential();
  }

  private scheduleRetry(): void {
    if (this.closed) return;
    // A retry is already pending, or we are already connected.
    const state = this.ws?.readyState;
    if (state === WebSocket.CONNECTING || state === WebSocket.OPEN) return;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = window.setTimeout(() => this.connect(), this.backoff);
    this.backoff = Math.min(this.backoff * 2, BACKOFF_MAX_MS);
  }

  get connected(): boolean {
    return this.authed && this.ws?.readyState === WebSocket.OPEN;
  }

  sendSamples(samples: readonly TouchSample[]): void {
    if (!samples.length || !this.connected) return;
    this.ws!.send(encodeFrame(this.thin(samples)));
  }

  /**
   * Drop stale intermediate positions when the socket is falling behind.
   *
   * Only `Move` samples are ever dropped, and never the last of them: a move is
   * a position, and an older position is worth nothing once a newer one exists.
   * Every phase change survives untouched, which is the rule that matters -
   * a lost `Up` is a stuck button, a lost `Down` is a gesture the desktop never
   * sees the start of and so never grants the cursor for. Those are the two
   * failures the immediate flushes in `surface.ts` exist to prevent, and
   * thinning must not reintroduce them by the back door.
   *
   * Normal conditions never reach this: `bufferedAmount` sits at zero when the
   * link is keeping up, so the whole batch goes out as it always did.
   */
  private thin(samples: readonly TouchSample[]): readonly TouchSample[] {
    if ((this.ws?.bufferedAmount ?? 0) <= MAX_BUFFERED_BYTES) return samples;
    // Backwards, so "is there a newer move than this one" is a flag rather than
    // a scan of the rest of the batch for every sample in it.
    const kept: TouchSample[] = [];
    let seenMove = false;
    for (let i = samples.length - 1; i >= 0; i--) {
      const s = samples[i];
      if (s.phase === Phase.Move) {
        if (seenMove) continue;
        seenMove = true;
      }
      kept.push(s);
    }
    return kept.reverse();
  }

  sendJson(msg: unknown): void {
    if (!this.connected) return;
    this.ws!.send(JSON.stringify(msg));
  }

  /**
   * Announce this device: how big its surface is, and what to call it.
   *
   * The name is the whole reason the other phone can say "iPad is using this
   * computer" instead of leaving its user to guess why nothing moves, so it goes
   * out with the geometry rather than in a message of its own that an older
   * desktop would have to know about.
   */
  sendWelcome(): void {
    this.sendJson({ t: "welcome", v: 1, surface: this.geometry, name: deviceName() });
  }
}
