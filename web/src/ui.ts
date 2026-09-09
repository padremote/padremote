/**
 * Status readout, settings sheet and touch feedback (plan.md section 10).
 *
 * Deliberately minimal: the surface is the product, so the chrome stays out of
 * the way and never steals a touch from it.
 */

import type { Status } from "./net";
import { deviceName, setDeviceName } from "./device";
import { primeClick, clickDown } from "./sound";
import { shakeSupport } from "./shake";

/**
 * What this phone has chosen for itself.
 *
 * `null` means "whatever the computer says", and it is the default for
 * everything the computer has an opinion about. That distinction is the whole
 * point: the page used to store a *value* for each setting and send it the
 * moment it connected, so the sheet's own defaults silently overrode the
 * computer's real trackpad settings - a Mac with natural scrolling off scrolled
 * the wrong way, and nothing on the desktop was wrong.
 */
export interface SettingsValues {
  sensitivity: number | null;
  naturalScroll: boolean | null;
  /**
   * Play a click when a drag starts and ends.
   *
   * Local to the phone - the desktop neither knows nor cares, so it has a real
   * default rather than a `null`. On everywhere, which it did not used to be:
   * it defaulted to `!hasVibration()`, so an Android phone started silent on the
   * grounds that it could buzz instead. It often cannot. Chrome refuses
   * `navigator.vibrate` unless the page holds a user activation from a
   * *completed* tap, and a long press never lifts the finger - so the one
   * gesture that most needs announcing is the one most likely to get nothing.
   * A phone that buzzes as well simply gets both, the way a real trackpad both
   * clicks and is felt, and the switch is right there for anyone who wants
   * silence.
   */
  clickSound: boolean;
}

/** A settings change to send to the desktop. */
export interface SettingsPatch {
  sensitivity?: number;
  naturalScroll?: boolean;
  /** Settings being handed back to the computer, by name. */
  follow?: string[];
}

/** What the desktop says this phone is actually being driven with. */
export interface EffectiveSettings {
  sensitivity: number;
  naturalScroll: boolean;
  following: { sensitivity: boolean; naturalScroll: boolean };
}

export interface UiHooks {
  onSettingsChange: (patch: SettingsPatch) => void;
  /** The user asked to take the cursor back from whoever holds it. */
  onTakeControl: () => void;
  /** This device was renamed, so the desktop and the other devices need telling. */
  onNameChange: (name: string) => void;
  /** Ask iOS for motion access. Resolves to whether shake now works. */
  onEnableShake: () => Promise<boolean>;
  /** The button version of a shake, for anything that cannot shake. */
  onFullScreen: () => void;
}

/** Who is driving the shared cursor, as the desktop last described it. */
export interface ControlState {
  active: boolean;
  holder?: string;
  devices: number;
  /** Why the computer is moving nothing for anybody. Absent when it is fine. */
  blocked?: "permission" | "dryRun";
}

/**
 * Bumped from `v1`, and it had to be.
 *
 * A v1 record stored a *value* for every setting, so a phone that had never
 * touched the sheet is indistinguishable from one that deliberately chose the
 * defaults - and under the rules this version plays by, reading one back would
 * mean overriding the computer's trackpad settings with a choice the user never
 * made. Which is the exact bug this key change is part of fixing. Dropping v1
 * costs a phone nothing it chose on purpose, and hands every existing one back
 * to the computer.
 */
const SETTINGS_KEY = "padremote.settings.v2";

function defaults(): SettingsValues {
  return { sensitivity: null, naturalScroll: null, clickSound: true };
}

function loadSettings(): SettingsValues {
  const settings = defaults();
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    const saved: unknown = raw ? JSON.parse(raw) : null;
    if (saved && typeof saved === "object") {
      const values = saved as Partial<SettingsValues>;
      if (typeof values.sensitivity === "number" && Number.isFinite(values.sensitivity)) {
        settings.sensitivity = Math.min(3, Math.max(0.25, values.sensitivity));
      }
      if (typeof values.naturalScroll === "boolean") settings.naturalScroll = values.naturalScroll;
      if (typeof values.clickSound === "boolean") settings.clickSound = values.clickSound;
    }
  } catch {
    /* storage unavailable; defaults are fine */
  }
  return settings;
}

function saveSettings(v: SettingsValues): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(v));
  } catch {
    /* ignore */
  }
}

/**
 * What a per-device override says while it is not overriding anything.
 *
 * Exported because `index.html` hard-codes the same words for the first paint,
 * and `scripts/check-mobile-ui.mjs` holds the two together.
 */
export const FOLLOWING = "Matching your computer";

export class Ui {
  private readonly dot = document.getElementById("dot")!;
  private readonly statusText = document.getElementById("status-text")!;
  private readonly gestureEl = document.getElementById("gesture")!;
  private readonly latencyEl = document.getElementById("latency")!;
  private readonly rateEl = document.getElementById("rate")!;
  private readonly jitterEl = document.getElementById("jitter")!;
  private readonly hint = document.getElementById("hint")!;
  private readonly takeControl = document.getElementById("take-control") as HTMLButtonElement;
  private readonly peersEl = document.getElementById("peers")!;
  private readonly sheet = document.getElementById("sheet")!;
  private readonly settingsOpen = document.getElementById("settings-open") as HTMLButtonElement;
  private readonly settingsClose = document.getElementById("settings-close") as HTMLButtonElement;
  private readonly backgroundInert = new Map<HTMLElement, boolean>();
  private sheetOpen = false;
  private readonly nameInput = document.getElementById("device-name") as HTMLInputElement;
  private readonly sensitivityInput = document.getElementById("sensitivity") as HTMLInputElement;
  private readonly sensitivityOut = document.getElementById("sensitivity-value")!;
  private readonly naturalInput = document.getElementById("natural") as HTMLInputElement;
  private readonly clickInput = document.getElementById("click-sound") as HTMLInputElement;
  private readonly shakeButton = document.getElementById("enable-shake") as HTMLButtonElement;
  private readonly shakeNote = document.getElementById("shake-note")!;
  private readonly fullScreenNote = document.getElementById("fullscreen-note")!;
  private readonly fullScreenButton = document.getElementById("go-full-screen") as HTMLButtonElement;
  private readonly sensitivityNote = document.getElementById("sensitivity-note") as HTMLButtonElement;
  private readonly naturalNote = document.getElementById("natural-note") as HTMLButtonElement;

  settings: SettingsValues = loadSettings();
  /** The vibration warning is worth saying once, and insufferable twice. */
  private hapticsNoted = false;
  /** The last-rendered link state, so `setControl` can redraw without it. */
  private status: Status = "connecting";
  private hostLabel = "";
  /** Assume we are driving until told otherwise: one device is the common case,
   *  and a page that opens by apologising for a queue that does not exist is
   *  worse than one that corrects itself on the first `control` message. */
  private control: ControlState = { active: true, devices: 1 };
  /**
   * What the computer says these settings actually are.
   *
   * Until it has said, the sheet shows nothing it could be wrong about: this
   * page has no way to know how the user's trackpad is set, and guessing is
   * exactly what went wrong before.
   */
  private effective: EffectiveSettings | null = null;

  constructor(private readonly hooks: UiHooks) {
    // Local controls are ready before any connection or permission request.
    this.sheet.hidden = true;
    this.sheet.setAttribute("aria-hidden", "true");
    this.settingsOpen.setAttribute("aria-controls", "sheet");
    this.settingsOpen.setAttribute("aria-expanded", "false");
    this.settingsOpen.addEventListener("click", () => this.toggleSheet(true));
    this.settingsClose.addEventListener("click", () => this.toggleSheet(false));
    this.sheet.addEventListener("click", (event) => {
      if (event.target === this.sheet) this.toggleSheet(false);
    });
    document.addEventListener("keydown", this.onSheetKeyDown);

    this.clickInput.checked = this.settings.clickSound;
    this.renderSettings();

    this.takeControl.addEventListener("click", () => this.hooks.onTakeControl());

    this.fullScreenButton.addEventListener("click", () => {
      this.toggleSheet(false);
      this.hooks.onFullScreen();
    });
    this.shakeButton.addEventListener("click", async () => {
      this.shakeButton.disabled = true;
      try {
        const granted = await this.hooks.onEnableShake();
        this.setShake(granted ? "ready" : "denied");
      } catch {
        this.setShake("denied");
      }
    });
    this.setShake(shakeSupport());

    this.nameInput.value = deviceName();
    this.nameInput.addEventListener("change", () => {
      setDeviceName(this.nameInput.value);
      // Show what was actually stored - trimmed, and back to the guess if the
      // field was cleared - so the field never disagrees with the other phone.
      this.nameInput.value = deviceName();
      this.hooks.onNameChange(this.nameInput.value);
    });

    this.sensitivityInput.addEventListener("input", () => {
      const value = Number(this.sensitivityInput.value);
      this.settings.sensitivity = value;
      this.commit({ sensitivity: value });
    });
    this.naturalInput.addEventListener("change", () => {
      const value = this.naturalInput.checked;
      this.settings.naturalScroll = value;
      this.commit({ naturalScroll: value });
    });

    // Handing a setting back to the computer. Offered only once this phone has
    // taken it over, because "match my computer" on a setting that already does
    // is a button that appears to do nothing.
    this.sensitivityNote.addEventListener("click", () => {
      if (this.settings.sensitivity === null) return;
      this.settings.sensitivity = null;
      this.commit({ follow: ["sensitivity"] });
    });
    this.naturalNote.addEventListener("click", () => {
      if (this.settings.naturalScroll === null) return;
      this.settings.naturalScroll = null;
      this.commit({ follow: ["naturalScroll"] });
    });
    this.clickInput.addEventListener("change", () => {
      this.settings.clickSound = this.clickInput.checked;
      // Switching it on is a user gesture, which is the one moment audio is
      // allowed to start - and playing the click here both unlocks the context
      // and lets the user hear what they just turned on.
      if (this.settings.clickSound) {
        primeClick();
        clickDown();
      }
      this.commit();
    });
  }

  private commit(patch?: SettingsPatch): void {
    saveSettings(this.settings);
    this.renderSettings();
    if (patch) this.hooks.onSettingsChange(patch);
  }

  /**
   * The settings the desktop is really driving this phone with.
   *
   * The computer is the source of truth - it mirrors the user's own trackpad -
   * so the sheet is filled in from here rather than from anything stored on the
   * phone. What the phone stores is only which settings it has *taken over*.
   */
  setEffective(effective: EffectiveSettings): void {
    this.effective = effective;
    // Trust the desktop about what is overridden: a stored override that the
    // desktop has forgotten (an older build, a restart) would otherwise leave
    // the sheet claiming a control is overridden when it is not.
    if (effective.following.sensitivity) this.settings.sensitivity = null;
    if (effective.following.naturalScroll) this.settings.naturalScroll = null;
    saveSettings(this.settings);
    this.renderSettings();
  }

  private renderSettings(): void {
    const e = this.effective;
    const sensitivity = this.settings.sensitivity ?? e?.sensitivity ?? 1;
    const natural = this.settings.naturalScroll ?? e?.naturalScroll ?? true;

    this.sensitivityInput.value = String(sensitivity);
    this.sensitivityOut.textContent = sensitivity.toFixed(2);
    this.naturalInput.checked = natural;

    // Each control says where its value came from. Without it "why is this on?"
    // has no answer anywhere on the phone, and the answer is usually "because
    // that is how your trackpad is set".
    const note = (el: HTMLButtonElement, following: boolean) => {
      // The following text has to be the same words `index.html` paints before
      // this runs, or the label visibly changes on load for no reason. The
      // other is worded as the action it performs: "Reset" reads as though it
      // throws something away, when it only hands the setting back.
      el.textContent = following ? FOLLOWING : "Use my computer’s setting";
      el.disabled = following;
    };
    note(this.sensitivityNote, this.settings.sensitivity === null);
    note(this.naturalNote, this.settings.naturalScroll === null);
  }

  private toggleSheet(open: boolean): void {
    if (open === this.sheetOpen) return;
    this.sheetOpen = open;
    this.settingsOpen.setAttribute("aria-expanded", String(open));
    if (open) {
      this.sheet.hidden = false;
      this.sheet.removeAttribute("aria-hidden");
      // The surrounding pad must not receive touches or keyboard focus while
      // the dialog is open. Restore each sibling's own state when it closes.
      for (const sibling of this.sheet.parentElement?.children ?? []) {
        if (!(sibling instanceof HTMLElement) || sibling === this.sheet) continue;
        this.backgroundInert.set(sibling, sibling.inert);
        sibling.inert = true;
      }
    } else {
      for (const [element, inert] of this.backgroundInert) element.inert = inert;
      this.backgroundInert.clear();
    }
    this.sheet.classList.toggle("open", open);
    if (open) {
      // Focusing a text field here would open the iPhone keyboard immediately.
      this.settingsClose.focus({ preventScroll: true });
    } else {
      this.settingsOpen.focus({ preventScroll: true });
      this.sheet.setAttribute("aria-hidden", "true");
      this.sheet.hidden = true;
    }
  }

  private onSheetKeyDown = (event: KeyboardEvent): void => {
    if (!this.sheetOpen) return;
    if (event.key === "Escape") {
      event.preventDefault();
      this.toggleSheet(false);
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(this.sheet.querySelectorAll<HTMLElement>(
      'button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), ' +
      'textarea:not(:disabled), summary, [tabindex]:not([tabindex="-1"])',
    )).filter((element) => element.getClientRects().length > 0);
    const first = controls[0] ?? this.sheet;
    const last = controls[controls.length - 1] ?? this.sheet;
    if (event.shiftKey && (document.activeElement === first || !this.sheet.contains(document.activeElement))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (document.activeElement === last || !this.sheet.contains(document.activeElement))) {
      event.preventDefault();
      first.focus();
    }
  };

  destroy(): void {
    this.toggleSheet(false);
    document.removeEventListener("keydown", this.onSheetKeyDown);
  }

  /**
   * Whether this phone has ever been told where the computer is.
   *
   * Off by default, so a page that never says otherwise gets the advice for a
   * phone that has not been paired - which is the state it is actually in.
   */
  private paired = false;

  /** Called once the address has been resolved, with where it came from. */
  setPaired(paired: boolean): void {
    this.paired = paired;
    this.render();
  }

  setStatus(status: Status, hostLabel: string): void {
    this.status = status;
    this.hostLabel = hostLabel;
    // A fresh link says nothing about whose turn it is until the desktop does.
    if (status !== "connected") this.control = { active: true, devices: 1 };
    this.render();
  }

  /**
   * Who has the cursor.
   *
   * This is the difference between "your turn is coming" and "this thing is
   * broken". The devices take turns automatically - touching the pad claims the
   * cursor as soon as whoever had it stops - so the message says exactly that
   * and offers no button: there is nothing to press.
   */
  setControl(control: ControlState): void {
    this.control = control;
    this.render();
  }

  /**
   * The line under the dot: where this trackpad stands, in three words.
   *
   * A `switch` rather than the ternary chain this used to be. Every new link
   * state - being un-paired, being replaced by another tab - added a rung to a
   * ladder that was already hard to read, and the version of this code that
   * was one state out of alignment still compiled and still ran.
   */
  private statusLine(): string {
    switch (this.status) {
      case "connected":
        return this.hostLabel;
      case "connecting":
        return "Connecting to your computer…";
      case "superseded":
        return "Connection paused";
      case "unpaired":
        return "Not paired yet";
      case "replaced":
        return "Open somewhere else";
      case "offline":
        return "Computer unavailable";
    }
  }

  /**
   * What to do about it, when there is anything to do.
   *
   * Empty means the pad is working and needs no explaining, which is the case
   * this whole readout should stay out of the way of.
   */
  private hintLine(waiting: boolean): string {
    switch (this.status) {
      case "connecting":
        return "Looking for your computer…";
      case "replaced":
        return "Open in another tab. Reload to take it back.";
      case "unpaired":
        return "Scan the QR code on your computer.";
      case "superseded":
        return `Another phone is controlling ${this.hostLabel}.`;
      case "offline":
        // A phone that has never been paired needs different advice from one
        // that simply cannot reach a computer it already knows.
        return this.paired
          ? "Check PadRemote is running, on the same Wi-Fi."
          : "Scan the QR code on your computer.";
      case "connected":
        return (
          this.blockedLine() ||
          (waiting ? `${this.control.holder} is in control. Touch to take over.` : "")
        );
    }
  }

  /**
   * What the computer says is stopping it moving anything, in words.
   *
   * This is the one failure that looks exactly like a healthy connection from
   * here - the socket is up, the gesture readout follows every finger, the
   * latency figure is live, and the cursor sits still - so it outranks whose
   * turn it is. When nothing moves for anybody, the queue is beside the point.
   */
  private blockedLine(): string {
    switch (this.control.blocked) {
      case "permission":
        return `Allow PadRemote in System Settings › Privacy & Security › Accessibility on ${this.hostLabel}.`;
      case "dryRun":
        return `${this.hostLabel} is in --dry-run. Restart it without that flag.`;
      default:
        return "";
    }
  }

  private render(): void {
    // A computer that can move nothing outranks whose turn it is: there is no
    // queue to be at the back of when the cursor is frozen for everyone.
    const frozen = this.status === "connected" && !!this.control.blocked;
    const waiting =
      this.status === "connected" && !frozen && !this.control.active && !!this.control.holder;
    // `dot` is the shared indicator from theme.css; the second class is the
    // state. Assigning the state alone would drop the shape. A frozen computer
    // must not show green: the dot is the fastest thing on this page to read,
    // and green over a dead cursor is the lie that sends people to their Wi-Fi.
    this.dot.className = `dot ${frozen || waiting ? "waiting" : this.status}`;
    this.statusText.textContent = this.statusLine();

    // Say how many devices are sharing this computer, and only then: on the
    // usual one-phone setup the count is noise.
    this.peersEl.hidden = this.control.devices < 2;
    this.peersEl.textContent = `${this.control.devices} devices`;

    // Only offered when this page has actually been evicted by an older
    // desktop: a button that reconnects would otherwise just be a second,
    // confusing retry control. Never for `replaced`, where the other tab is on
    // this same phone and the two would only take the link off each other.
    this.takeControl.hidden = this.status !== "superseded";

    const hint = this.hintLine(waiting);
    this.hint.textContent = hint;
    this.hint.hidden = hint === "";
  }

  /**
   * Whether shaking the phone does anything, and what to say about it.
   *
   * Four states rather than a checkbox, because "off" here has four different
   * meanings and only one of them is something the user can fix.
   */
  private setShake(
    state: "insecure" | "unavailable" | "needs-permission" | "ready" | "denied",
  ): void {
    this.shakeButton.hidden = state !== "needs-permission";
    this.shakeButton.disabled = false;
    // The row itself always stays: the button beside it works regardless, and
    // hiding the explanation is how "why doesn't shake do anything?" becomes
    // unanswerable.
    this.shakeNote.textContent = {
      ready: "shake the phone to toggle full screen",
      "needs-permission": "needs motion access",
      denied: "Motion access denied — use the button.",
      // Not a phone setting and not something the user did wrong: every
      // browser gates motion behind https, and this page is served over plain
      // http until PadRemote grows a certificate.
      insecure: "shake needs https — use the button",
      unavailable: "no motion sensor — use the button",
    }[state];
  }

  /**
   * Say, permanently, when this browser cannot give a page the whole screen.
   *
   * iOS exposes no Fullscreen API to any browser - Chrome and Safari alike are
   * WebKit, and Apple has only ever shipped it for `<video>`, which is why a
   * YouTube video can do it and a web page cannot. Hiding the app's own
   * controls is all that is left, and someone who asked for full screen and got
   * a browser bar deserves to be told why and what would actually work, in the
   * place the control lives rather than in a message that fades in three
   * seconds.
   */
  setFullScreenLimit(limited: boolean): void {
    this.fullScreenNote.hidden = !limited;
    this.fullScreenNote.textContent = limited
      ? "Add to Home Screen (Share ▸ Add to Home Screen) to lose the browser bars."
      : "";
  }

  /**
   * Say something happened, briefly, without opening the sheet.
   *
   * Full screen is a mode change with no other visible confirmation - and on
   * an iPhone what actually happened is not what was asked for, so it has to
   * be able to explain itself in a sentence.
   */
  flash(text: string): void {
    this.hint.hidden = false;
    this.hint.textContent = text;
    window.setTimeout(() => {
      if (this.hint.textContent === text) this.render();
    }, 3200);
  }

  setGesture(name: string): void {
    this.gestureEl.textContent = name;
  }

  /**
   * Say, once, that the browser refused to vibrate.
   *
   * Chrome grants a page its user activation from a *completed* tap, and a long
   * press buzzes with the finger still down - so on a freshly loaded page whose
   * first gesture is a hold, the vibration is refused and nothing anywhere says
   * why. One tap fixes it for the life of the page; the user just has to be
   * told that. Shown only while connected, where the hint line is otherwise
   * idle, and never repeated.
   */
  noteHapticsBlocked(): void {
    if (this.hapticsNoted) return;
    this.hapticsNoted = true;
    this.hint.hidden = false;
    this.hint.textContent = "Tap the pad once to let this browser vibrate.";
    setTimeout(() => {
      if (this.hint.textContent?.startsWith("Tap the pad once")) this.hint.hidden = true;
    }, 4000);
  }

  setLatency(ms: number): void {
    this.latencyEl.textContent = `${Math.max(0, Math.round(ms))} ms`;
  }

  setRate(hz: number): void {
    this.rateEl.textContent = `${hz} Hz`;
  }

  /**
   * Spread between the fastest and slowest frame in the last window.
   *
   * This is the number that explains stutter: a steady 8 ms cadence feels
   * smooth, while an average of 8 ms that swings between 2 ms and 40 ms does
   * not, and an average alone would hide that entirely.
   */
  setJitter(ms: number): void {
    this.jitterEl.textContent = `±${Math.round(ms)} ms`;
  }
}

/** A ripple where the finger landed, so the surface feels alive. */
export function blip(parent: HTMLElement, x: number, y: number): void {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const el = document.createElement("div");
  const bounds = parent.getBoundingClientRect();
  // The negative margins put the circle's *centre* on the fingertip; the left
  // and top set below are the touch point itself.
  el.className = "pointer-events-none absolute -ml-[22px] -mt-[22px] h-11 w-11 " +
    "rounded-full border border-[#93a7ff66] bg-[#93a7ff0d]";
  el.style.left = `${x - bounds.left}px`;
  el.style.top = `${y - bounds.top}px`;
  parent.appendChild(el);
  el.animate(
    [
      { opacity: 0.9, transform: "scale(0.5)" },
      { opacity: 0, transform: "scale(1.4)" },
    ],
    { duration: 380, easing: "ease-out" },
  ).onfinish = () => el.remove();
}
