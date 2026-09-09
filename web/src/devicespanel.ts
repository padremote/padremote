/**
 * The Connected devices page.
 *
 * The desktop has always known which phones are paired with it - the list is
 * what makes revoking one phone different from revoking all of them - and it
 * has always served that list on `/devices`, to the connect screen the computer
 * shows while it is drawing a QR code. Which is the one screen the person
 * holding the phone is not looking at.
 *
 * So the same channel feeds the settings page. Two rules come with it, both
 * enforced on the desktop rather than here (`net/devices.rs`):
 *
 * - **Reading** is for anyone who has answered the challenge. A phone already
 *   knows it is paired; learning that the iPad is too tells it nothing new.
 * - **Un-pairing** is for the computer alone, decided from the TCP peer being
 *   loopback. The desktop says so in `manage`, and a page that hid the buttons
 *   without being told would be guessing - so the list is read-only on a phone
 *   and says why, rather than offering a button that would be refused.
 */

import { authReply, socketUrl } from "./pairing";

const $ = (id: string) => document.getElementById(id)!;

interface Device {
  id: string | null;
  name: string;
  connected: boolean;
  driving: boolean;
  since: number | null;
}

interface DeviceState {
  t: "devices";
  connected: number;
  manage: boolean;
  devices: Device[];
}

let ws: WebSocket | null = null;
let backoff = 500;
/** Set once a challenge went unanswered; retrying would only repeat it. */
let unpaired = false;
let manage = false;

/**
 * When the phone was paired, in a form a person reads.
 *
 * The desktop sends seconds since the epoch rather than a formatted date, so
 * that the file it keeps does not depend on a locale to be read - which leaves
 * the formatting here, where there is a locale to read it in.
 */
function paired(since: number | null): string {
  if (!since) return "";
  return `Paired ${new Date(since * 1000).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  })}`;
}

/** What one device is doing, in the order it matters. */
function doing(d: Device): string {
  const parts = [];
  if (d.driving) parts.push("Moving the cursor now");
  else if (d.connected) parts.push("Connected");
  const when = paired(d.since);
  if (when) parts.push(when);
  return parts.join(" · ");
}

function row(d: Device): HTMLElement {
  const li = document.createElement("li");
  li.className = "device grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[13px] " +
    "min-h-14 border-t border-line py-[13px] first:border-t-0 first:pt-0.5 last:pb-0.5";
  if (d.connected) li.classList.add("is-connected");

  const dot = document.createElement("span");
  // Grey rather than red when it is not here. A phone in a bag downstairs is
  // paired and switched off, which is not a fault - and a list of red dots for
  // every device someone owns is a page reporting an emergency every time.
  dot.className = `dot${d.connected ? " connected" : ""}`;
  dot.setAttribute("aria-hidden", "true");

  const words = document.createElement("span");
  // Connected is the state worth reading off the list at a glance, so the row
  // that is live says so in the page's own colour rather than only in a dot.
  words.className = "device-words grid min-w-0 gap-[5px] [&>b]:font-[550] [&>b]:[overflow-wrap:anywhere] " +
    "[&>small]:text-[12.5px] [&>small]:text-faint [.is-connected_&>small]:text-ok";
  const name = document.createElement("b");
  name.textContent = d.name || "Unnamed device";
  const what = document.createElement("small");
  what.textContent = doing(d);
  words.append(name, what);
  li.append(dot, words);

  // No id means no credential to revoke - the replay tool and the tests
  // authenticate with the QR secret and never enrol. Listing them keeps the
  // count honest; offering to forget one would be offering nothing.
  if (manage && d.id) {
    const forget = document.createElement("button");
    forget.type = "button";
    forget.className = "btn btn-bare min-h-[38px] px-[13px] py-[7px]";
    forget.dataset.forget = d.id;
    forget.textContent = "Forget";
    forget.addEventListener("click", () => send({ t: "forget", id: d.id }));
    li.append(forget);
  }
  return li;
}

function render(s: DeviceState): void {
  manage = s.manage;
  const rows = $("device-rows");
  rows.replaceChildren(...s.devices.map(row));
  $("device-empty").hidden = s.devices.length > 0;
  // The count sits in the section's own heading, on the home page, beside the
  // list it counts. It used to be a card's subtitle standing in for a list that
  // was one tap away; the list is here now, so the number is a caption.
  $("device-count").textContent = s.devices.length
    ? `${s.connected} of ${s.devices.length} connected`
    : "";
  // Said once, under the list, rather than as a disabled button per row: the
  // phone cannot revoke anything, and a row of greyed-out buttons is a page
  // asking to be tapped before it will explain itself.
  $("device-note").hidden = manage || s.devices.length === 0;
  $("forget-panel").hidden = !manage || s.devices.length === 0;
}

function send(message: unknown): void {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(message));
}

/**
 * Watch the paired devices.
 *
 * Its own socket, on its own address: `/devices` and `/config` are different
 * channels with different rules about who may write to them, and merging them
 * into one connection would mean one of the two rules being enforced by this
 * page rather than by the desktop.
 */
export function startDevices(link: { host: string; key?: string }): void {
  const connect = (): void => {
    ws = new WebSocket(`${socketUrl({ host: link.host })}/devices`);
    ws.onclose = () => {
      ws = null;
      if (unpaired) return;
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 5000);
    };
    ws.onopen = () => {
      backoff = 500;
    };
    ws.onerror = () => ws?.close();
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data as string) as
        | DeviceState
        | { t: "error"; detail: string }
        | { t: "challenge"; nonce: string };
      if (msg.t === "challenge") {
        const reply = authReply(msg.nonce, link.key);
        if (!reply) {
          // The settings socket says the same thing in the header, where it is
          // already being read. Saying it twice on a page that cannot connect
          // at all is not twice as useful.
          unpaired = true;
          ws?.close();
          return;
        }
        ws?.send(JSON.stringify({ t: "auth", ...reply }));
        return;
      }
      if (msg.t === "error") return;
      render(msg);
    };
  };
  connect();

  // Forgetting everything is the one action here that cannot be undone, so it
  // is asked in the page for the same reason Restore defaults is: a native
  // `confirm()` on a phone is a modal this page cannot style, cannot place away
  // from a thumb, and that some browsers decline to show at all.
  $("forget-all").addEventListener("click", () => {
    $("forget-confirm").hidden = false;
    $("forget-all").hidden = true;
    $("forget-yes").focus();
  });
  $("forget-no").addEventListener("click", close);
  $("forget-yes").addEventListener("click", () => {
    close();
    send({ t: "forgetAll" });
  });
  function close(): void {
    $("forget-confirm").hidden = true;
    $("forget-all").hidden = false;
  }
}
