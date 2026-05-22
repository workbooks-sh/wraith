// Ed25519-sign a binary with the private key in ED25519_BINARY_SIGNING_PRIVATE_KEY.
//
// The secret is expected to hold the 32-byte Ed25519 private key seed encoded
// as base64 (not PKCS8). This matches ed25519-dalek's SigningKey::to_bytes()
// and keeps the workflow secret compact and encoding-agnostic.
//
// Usage:
//   node sign-binary.mjs <input-binary> <output-sig>

import { readFileSync, writeFileSync } from "node:fs";
import { createPrivateKey, sign } from "node:crypto";

const [, , inputPath, outputPath] = process.argv;
if (!inputPath || !outputPath) {
  console.error("usage: sign-binary.mjs <input-binary> <output-sig>");
  process.exit(1);
}

const seedB64 = process.env.ED25519_BINARY_SIGNING_PRIVATE_KEY;
if (!seedB64) {
  console.error("ED25519_BINARY_SIGNING_PRIVATE_KEY env var is required");
  process.exit(1);
}

const seed = Buffer.from(seedB64.trim(), "base64");
if (seed.length !== 32) {
  console.error(`expected 32-byte Ed25519 seed, got ${seed.length} bytes`);
  process.exit(1);
}

const pkcs8Header = Buffer.from([
  0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
]);
const pkcs8 = Buffer.concat([pkcs8Header, seed]);
const privateKey = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });

const data = readFileSync(inputPath);
const signature = sign(null, data, privateKey);

if (signature.length !== 64) {
  console.error(`unexpected signature length ${signature.length} (want 64)`);
  process.exit(1);
}

writeFileSync(outputPath, signature);
console.log(`wrote ${signature.length}-byte signature to ${outputPath}`);
