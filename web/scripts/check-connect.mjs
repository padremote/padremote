/*
 * Exercise the real inline controller of the desktop's connect page.
 *
 * The page is served by the app rather than built by Vite - only the app can
 * draw the QR and hand the page the pairing secret - so nothing else here
 * type-checks or tests it. This runs the actual script out of the actual file
 * against a fake DOM and a fake socket, which is the only way the device list,
 * the two-tap Forget all, and the stale-address reload get covered at all.
 */
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const KEY = '0123456789abcdef0123456789abcdef';
const source = readFileSync(new URL('../../desktop/src/assets/connect.html', import.meta.url), 'utf8');
const script = source
  .split('<script>')[1]
  .split('</script>')[0]
  .replaceAll('__KEY__', KEY)
  .replaceAll('__IP__', '192.168.1.24')
  .replaceAll('__PORT__', '8787');

/** Just enough of an element for the page to build rows out of. */
function element(id = '', hidden = false) {
  const el = {
    id,
    hidden,
    textContent: '',
    className: '',
    children: [],
    style: {},
    classList: {
      names: new Set(),
      add(n) { this.names.add(n); },
      remove(n) { this.names.delete(n); },
    },
    attributes: {},
    setAttribute(name, value) { el.attributes[name] = value; },
    append(...kids) { el.children.push(...kids); },
    replaceChildren(...kids) { el.children = kids; },
  };
  return el;
}

const byId = new Map(
  ['list', 'count', 'empty', 'forget-all', 'note', 'connect',
    'paired-intro', 'paired-who', 'show-qr'].map((id) => [id, element(id)]),
);
// The two the markup ships hidden, so the first render is asserted against the
// state a browser would actually start in.
for (const id of ['paired', 'paired-lost']) byId.set(id, element(id, true));
const list = byId.get('list');
const count = byId.get('count');
const forgetAll = byId.get('forget-all');
const note = byId.get('note');
const connectCard = byId.get('connect');
const pairedCard = byId.get('paired');

let sent = [];
let reloaded = 0;
let socket;
class FakeSocket {
  static OPEN = 1;
  constructor(url) {
    this.url = url;
    this.readyState = 1;
    socket = this;
  }
  send(text) { sent.push(JSON.parse(text)); }
  close() { this.readyState = 3; this.onclose?.(); }
}

const timers = [];
vm.runInNewContext(script, {
  document: { getElementById: (id) => byId.get(id), createElement: () => element() },
  window: { isSecureContext: true },
  location: { host: 'localhost:8787', reload: () => { reloaded += 1; } },
  crypto: globalThis.crypto,
  TextEncoder,
  WebSocket: FakeSocket,
  setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; },
  clearTimeout: () => {},
  JSON,
  console,
});

const deliver = async (message) => {
  await socket.onmessage({ data: JSON.stringify(message) });
};
const devices = (rows, extra = {}) => ({
  t: 'devices',
  connected: rows.filter((d) => d.connected).length,
  manage: true,
  host: '192.168.1.24',
  devices: rows,
  ...extra,
});
const row = (el) => ({
  name: el.children[0].children[0].textContent,
  state: el.children[0].children[1].textContent,
  forget: el.children[1],
});

assert.equal(socket.url, 'ws://localhost:8787/devices', 'the page talks to its own server');

// The challenge is answered with the same HMAC the desktop computes.
await deliver({ t: 'challenge', nonce: 'a-nonce' });
assert.deepEqual(sent, [
  { t: 'auth', hmac: createHmac('sha256', Buffer.from(KEY, 'hex')).update('a-nonce').digest('hex') },
]);
sent = [];

// Nothing paired: the invitation to scan, and no way to forget anything.
await deliver(devices([]));
assert.equal(count.textContent, 'Waiting for your phone…');
assert.equal(list.children.length, 0);
assert.equal(byId.get('empty').hidden, false);
assert.equal(forgetAll.hidden, true);
// Step one: nothing has connected, so the code is the whole page.
assert.equal(connectCard.hidden, false, 'the QR is the first step');
assert.equal(pairedCard.hidden, true, 'there is nothing to guide yet');

// One phone driving, one paired and away, and one connected without a
// credential of its own - the replay tool, which has nothing to revoke.
await deliver(
  devices([
    { id: 'aa', name: 'iPhone', connected: true, driving: true },
    { id: 'bb', name: 'Old iPad', connected: false, driving: false },
    { id: null, name: 'replay', connected: true, driving: false },
  ]),
);
assert.equal(count.textContent, '2 devices connected');
assert.equal(byId.get('empty').hidden, true);
const [phone, ipad, replay] = list.children.map(row);
assert.deepEqual([phone.name, phone.state], ['iPhone', 'Using the cursor']);
assert.deepEqual([ipad.name, ipad.state], ['Old iPad', 'Not connected']);
assert.equal(replay.forget, undefined, 'a device with no credential cannot be forgotten');

// Step two, and the whole point of it: a phone that has authenticated does not
// need the code it just scanned, it needs the thing to try and the settings.
assert.equal(connectCard.hidden, true, 'the QR gives way once a trackpad connects');
assert.equal(pairedCard.hidden, false, 'the guide takes its place');
assert.equal(byId.get('paired-who').textContent, 'iPhone', 'the guide names the device driving');
assert.equal(byId.get('paired-lost').hidden, true);

assert.equal(phone.forget.attributes['aria-label'], 'Forget iPhone', 'every row needs a distinct label');
phone.forget.onclick();
assert.deepEqual(sent, [{ t: 'forget', id: 'aa' }]);
sent = [];

// Forgetting everything revokes each device and replaces the code, so a single
// stray click must not be able to do it.
assert.equal(forgetAll.hidden, false);
forgetAll.onclick();
assert.deepEqual(sent, [], 'one click only arms it');
assert.match(forgetAll.textContent, /Click again/);
forgetAll.onclick();
assert.deepEqual(sent, [{ t: 'forgetAll' }]);
socket.close();
assert.equal(reloaded, 1, 'the page reloads to show the code the new secret produced');

// Paired but all away: the count says so rather than inviting a first scan.
await deliver(devices([{ id: 'bb', name: 'Old iPad', connected: false }]));
assert.equal(count.textContent, 'None connected');
// A phone whose screen locked has not un-paired. The guide stays, and says the
// one thing that is now true - showing the code again would have the user
// scanning a second time for a link that comes back on its own.
assert.equal(pairedCard.hidden, false, 'the guide survives a disconnect');
assert.equal(connectCard.hidden, true, 'a drop is not a reason to re-pair');
assert.equal(byId.get('paired-intro').hidden, true);
assert.equal(byId.get('paired-lost').hidden, false, 'a disconnect says so');

// Adding a second device is the one way back to the code, and it is the user's
// choice. It lasts until the device it was asked for turns up.
byId.get('show-qr').onclick();
assert.equal(connectCard.hidden, false, 'Add another device brings the code back');
assert.equal(pairedCard.hidden, true);
await deliver(devices([{ id: 'bb', name: 'Old iPad', connected: true, driving: true }]));
assert.equal(connectCard.hidden, true, 'the code goes away once it has been used');
assert.equal(byId.get('paired-who').textContent, 'Old iPad');

// A refusal is shown rather than swallowed.
sent = [];
await deliver({ t: 'error', detail: 'only this computer can change pairing' });
assert.equal(note.hidden, false);
assert.match(note.textContent, /only this computer/);

// A page that cannot manage devices offers no buttons at all.
await deliver(devices([{ id: 'aa', name: 'iPhone', connected: true }], { manage: false }));
assert.equal(list.children.length, 1);
assert.equal(row(list.children[0]).forget, undefined);
assert.equal(forgetAll.hidden, true);

// Forgetting the last device really does leave nothing paired, so the code -
// and the invitation under the list - belong together again.
await deliver(devices([]));
assert.equal(connectCard.hidden, false, 'an empty list is step one again');
assert.equal(pairedCard.hidden, true);

// The router hands out a new address: every code from before points at
// nothing, so the page reloads rather than showing a phone a dead QR.
reloaded = 0;
await deliver(devices([], { host: '192.168.1.99' }));
assert.equal(reloaded, 1, 'a moved address left a stale QR on screen');

console.log('ok  connect page: two steps, device list, forgetting, and a moved address');
