// Ed25519-verify a binary + signature pair against the public key published in
// ED25519_BINARY_SIGNING_PUBLIC_KEY.
//
// Usage:
//   node verify-binary.mjs <input-binary> <input-sig>

import { readFileSync } from "node:fs";
import { createPublicKey, verify } from "node:crypto";

const [, , binaryPath, sigPath] = process.argv;
if (!binaryPath || !sigPath) {
  console.error("usage: verify-binary.mjs <input-binary> <input-sig>");
  process.exit(1);
}

const pubB64 = process.env.ED25519_BINARY_SIGNING_PUBLIC_KEY;
if (!pubB64) {
  console.error("ED25519_BINARY_SIGNING_PUBLIC_KEY env var is required");
  process.exit(1);
}

const rawPub = Buffer.from(pubB64.trim(), "base64");
if (rawPub.length !== 32) {
  console.error(`expected 32-byte Ed25519 public key, got ${rawPub.length} bytes`);
  process.exit(1);
}

const spkiHeader = Buffer.from([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
]);
const spki = Buffer.concat([spkiHeader, rawPub]);
const publicKey = createPublicKey({ key: spki, format: "der", type: "spki" });

const data = readFileSync(binaryPath);
const signature = readFileSync(sigPath);

if (!verify(null, data, publicKey, signature)) {
  console.error(`ed25519 verification FAILED for ${binaryPath}`);
  process.exit(1);
}

console.log(`ed25519 verification ok: ${binaryPath}`);
