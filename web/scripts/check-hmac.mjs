/**
 * Is our SHA-256 the real SHA-256?
 *
 * `src/hmac.ts` implements the hash by hand, because the phone page is served
 * over plain http from a LAN address and `crypto.subtle` does not exist outside
 * a secure context. That is a defensible decision only if the implementation is
 * held to the same standard as any other, so it is checked here two ways:
 *
 * - against the **published vectors** (FIPS 180-4 for SHA-256, RFC 4231 for
 *   HMAC-SHA256), which is what every other implementation is checked against;
 * - **differentially against Node's `crypto`** - OpenSSL - over random inputs
 *   and, deliberately, every length around a block boundary. Padding at exactly
 *   55, 56, 63 and 64 bytes is where hand-written SHA-256 goes wrong, and a
 *   wrong answer there would still look perfectly plausible.
 *
 * If this fails, the phone cannot answer the desktop's pairing challenge and
 * nothing connects at all - so it fails loudly rather than subtly.
 *
 *   npm run check:hmac
 */

import { execFileSync } from "node:child_process";
import { createHash, createHmac, randomBytes } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const out = mkdtempSync(join(tmpdir(), "hmac-"));
const bundle = join(out, "hmac.mjs");
execFileSync(
  "npx",
  ["esbuild", "src/hmac.ts", "--bundle", "--format=esm", `--outfile=${bundle}`, "--log-level=error"],
  { stdio: "inherit" },
);
const { sha256, hmacSha256, answerChallenge, deriveDeviceKey, hex, unhex } =
  await import(bundle);

let failures = 0;
function check(name, got, want) {
  if (got === want) return;
  failures++;
  console.error(`FAIL ${name}\n  got  ${got}\n  want ${want}`);
}

const bytes = (s) => new TextEncoder().encode(s);
const repeat = (byte, n) => new Uint8Array(n).fill(byte);

// ---------------------------------------------------------------- FIPS 180-4

check("sha256('')", hex(sha256(bytes(""))),
  "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
check("sha256('abc')", hex(sha256(bytes("abc"))),
  "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
check("sha256(448-bit message)",
  hex(sha256(bytes("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))),
  "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
check("sha256(896-bit message)",
  hex(sha256(bytes(
    "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno" +
    "ijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"))),
  "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1");

// ----------------------------------------------------------------- RFC 4231

const rfc4231 = [
  [repeat(0x0b, 20), bytes("Hi There"),
   "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"],
  [bytes("Jefe"), bytes("what do ya want for nothing?"),
   "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"],
  [repeat(0xaa, 20), repeat(0xdd, 50),
   "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"],
  [Uint8Array.from({ length: 25 }, (_, i) => i + 1), repeat(0xcd, 50),
   "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b"],
  // The two cases with a key longer than the block, which is the branch that
  // hashes the key down first.
  [repeat(0xaa, 131), bytes("Test Using Larger Than Block-Size Key - Hash Key First"),
   "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"],
  [repeat(0xaa, 131), bytes(
    "This is a test using a larger than block-size key and a larger than " +
    "block-size data. The key needs to be hashed before being used by the " +
    "HMAC algorithm."),
   "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2"],
];
rfc4231.forEach(([key, msg, want], i) => {
  check(`RFC 4231 case ${i + 1}`, hex(hmacSha256(key, msg)), want);
});

// --------------------------------------------------- differential vs OpenSSL

// Every length where the padding decision changes, plus the two blocks around
// it. 55 and 56 are the classic off-by-one: at 56 the length no longer fits in
// the same block and a second one is required.
const boundaries = [0, 1, 54, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 129, 1000];
for (const n of boundaries) {
  const data = randomBytes(n);
  check(`sha256 of ${n} bytes`, hex(sha256(new Uint8Array(data))),
    createHash("sha256").update(data).digest("hex"));
}

for (let i = 0; i < 200; i++) {
  const key = randomBytes(1 + Math.floor(Math.random() * 200));
  const msg = randomBytes(Math.floor(Math.random() * 300));
  check(`hmac round ${i}`, hex(hmacSha256(new Uint8Array(key), new Uint8Array(msg))),
    createHmac("sha256", key).update(msg).digest("hex"));
}

// ------------------------------------------------------ the challenge answer

// The exact thing the phone sends back, against what the desktop will compute.
const secret = randomBytes(16).toString("hex");
const nonce = randomBytes(16).toString("hex");
check("answerChallenge", answerChallenge(secret, nonce),
  createHmac("sha256", Buffer.from(secret, "hex")).update(nonce).digest("hex"));

// A secret the page could not read is not an answer of "" - it is no answer.
check("a malformed secret has no answer", String(answerChallenge("not hex", nonce)), "null");
check("an odd-length secret has no answer", String(answerChallenge("abc", nonce)), "null");
check("unhex rejects junk", String(unhex("zz")), "null");

// ------------------------------------------------ agreement with the desktop

// The desktop derives this in `Secret::device_key` and pins the same expected
// value in `src/auth.rs`. Both are checked against Python's `hmac` rather than
// against each other, so neither implementation is grading itself. If these two
// drift apart, every device fails to pair with a bare `badAuth` and no test on
// either side alone would say why.
check("device key matches the desktop's",
  deriveDeviceKey("0f1e2d3c4b5a69788796a5b4c3d2e1f0", "11111111111111111111111111111111"),
  "f49d3689374a991e2595220d938faa9e");
check("a device key is 16 bytes", String(deriveDeviceKey(secret, "ab").length), "32");
check("two devices under one secret get different keys",
  String(deriveDeviceKey(secret, "aa") === deriveDeviceKey(secret, "bb")), "false");
check("a malformed secret derives nothing", String(deriveDeviceKey("nope", "aa")), "null");

rmSync(out, { recursive: true, force: true });
if (failures) {
  console.error(`\n${failures} check(s) failed - the phone cannot pair with this build.`);
  process.exit(1);
}
console.log("hmac: FIPS 180-4, RFC 4231, 215 differential cases and the desktop device key all match.");
