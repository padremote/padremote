/**
 * SHA-256 and HMAC-SHA256, for answering the desktop's pairing challenge.
 *
 * ## Why this is not `crypto.subtle`
 *
 * The obvious implementation is three lines of Web Crypto. It cannot be used
 * here: `crypto.subtle` exists only in a **secure context**, and the phone page
 * is served over plain `http://` from a LAN address, which is not one - only
 * `https://` and `localhost` qualify. On the device this app is actually for, a
 * phone loading `http://192.168.1.24:5173`, `crypto.subtle` is `undefined`.
 *
 * So the hash is implemented here, from FIPS 180-4. Writing your own crypto is
 * normally the wrong answer, and the reasons it is usually wrong mostly do not
 * apply to a hash: SHA-256 has no key-dependent branches to leak timing
 * through, no padding oracle, no nonce to reuse. What it does have is a hundred
 * ways to get subtly wrong, so `npm run check:hmac` checks it against the
 * published NIST and RFC 4231 vectors, and differentially against Node's own
 * `crypto` over random inputs and every block-boundary length.
 *
 * When the link moves to `wss://` (milestone 3's other half), the page becomes
 * a secure context and this file can become a fallback for `crypto.subtle`
 * rather than the only path.
 */

const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

const BLOCK_BYTES = 64;
const HASH_BYTES = 32;

function rotr(x: number, n: number): number {
  return (x >>> n) | (x << (32 - n));
}

/** SHA-256 of `data`, as 32 bytes. */
export function sha256(data: Uint8Array): Uint8Array {
  const h = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);

  // Padding: the message, a 1 bit, zeros, then the length in bits as a 64-bit
  // big-endian integer. The length lives in the last 8 bytes of the last block.
  const bitLen = data.length * 8;
  // Room for the message, the 0x80 byte and the 8-byte length, rounded up to a
  // whole number of blocks. `Math.ceil` and not a truncating shortcut: a
  // message whose padding lands exactly on a block boundary must not get a
  // spare block of zeros, which would change the hash.
  const padded = new Uint8Array(Math.ceil((data.length + 9) / BLOCK_BYTES) * BLOCK_BYTES);
  padded.set(data);
  padded[data.length] = 0x80;
  // Only the low 32 bits of the length are written. A message long enough to
  // need more than that is 512 MB, and nothing here hashes more than a few
  // hundred bytes; the alternative is BigInt on a hot path for no reason.
  const view = new DataView(padded.buffer);
  view.setUint32(padded.length - 4, bitLen >>> 0, false);
  view.setUint32(padded.length - 8, Math.floor(bitLen / 0x100000000), false);

  const w = new Uint32Array(64);
  for (let off = 0; off < padded.length; off += BLOCK_BYTES) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(off + i * 4, false);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }

    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (hh + s1 + ch + K[i] + w[i]) >>> 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (s0 + maj) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    h[0] = (h[0] + a) >>> 0;
    h[1] = (h[1] + b) >>> 0;
    h[2] = (h[2] + c) >>> 0;
    h[3] = (h[3] + d) >>> 0;
    h[4] = (h[4] + e) >>> 0;
    h[5] = (h[5] + f) >>> 0;
    h[6] = (h[6] + g) >>> 0;
    h[7] = (h[7] + hh) >>> 0;
  }

  const out = new Uint8Array(HASH_BYTES);
  const outView = new DataView(out.buffer);
  for (let i = 0; i < 8; i++) outView.setUint32(i * 4, h[i], false);
  return out;
}

/** HMAC-SHA256, per RFC 2104. */
export function hmacSha256(key: Uint8Array, message: Uint8Array): Uint8Array {
  // A key longer than the block is hashed down; a shorter one is zero-padded.
  const block = new Uint8Array(BLOCK_BYTES);
  block.set(key.length > BLOCK_BYTES ? sha256(key) : key);

  const inner = new Uint8Array(BLOCK_BYTES + message.length);
  const outer = new Uint8Array(BLOCK_BYTES + HASH_BYTES);
  for (let i = 0; i < BLOCK_BYTES; i++) {
    inner[i] = block[i] ^ 0x36;
    outer[i] = block[i] ^ 0x5c;
  }
  inner.set(message, BLOCK_BYTES);
  outer.set(sha256(inner), BLOCK_BYTES);
  return sha256(outer);
}

/** Hex to bytes; `null` for anything that is not an even run of hex digits. */
export function unhex(text: string): Uint8Array | null {
  if (!text || text.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(text)) return null;
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(text.substr(i * 2, 2), 16);
  return out;
}

export function hex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) out += b.toString(16).padStart(2, "0");
  return out;
}

/**
 * The answer to the desktop's challenge: hex `HMAC-SHA256(secret, nonce)`.
 *
 * The nonce is signed as the ASCII text the desktop sent, not as the bytes it
 * decodes to - both sides have to agree, and text is the thing that is
 * unambiguously on the wire.
 */
export function answerChallenge(secretHex: string, nonce: string): string | null {
  const key = unhex(secretHex);
  if (!key) return null;
  return hex(hmacSha256(key, new TextEncoder().encode(nonce)));
}

/**
 * This device's own key, derived from the QR's enrolment secret.
 *
 * The desktop derives the identical value in `Secret::device_key`, and the two
 * are pinned to the same third-party vector on both sides - if they ever drift
 * apart, every device fails to pair with a bare `badAuth` and neither side's
 * own tests would say why.
 *
 * Derived rather than exchanged, because the link is still plain `ws://`: a key
 * sent over it is a key an eavesdropper has. Both ends compute it from the
 * secret and the device's own (public) id, and nothing secret is ever sent.
 */
export function deriveDeviceKey(secretHex: string, deviceId: string): string | null {
  const secret = unhex(secretHex);
  if (!secret) return null;
  const message = new TextEncoder().encode(`padremote:device:${deviceId}`);
  // 16 bytes, the same length as the secret it came from.
  return hex(hmacSha256(secret, message).slice(0, 16));
}

/**
 * A fresh device id: 16 random bytes, hex.
 *
 * `crypto.getRandomValues` is available outside a secure context - unlike
 * `crypto.subtle`, which is the whole reason the hash above is hand-written.
 * The id is public anyway; it identifies a device, it does not authenticate
 * one, and the key derived from it is what does the work.
 */
export function newDeviceId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return hex(bytes);
}
