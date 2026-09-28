import { createHash, randomBytes } from "node:crypto";

// Port of crates/nori-gmail/src/pkce.rs — RFC 7636 S256.
export interface Pkce {
  verifier: string;
  challenge: string;
  state: string;
}

export function generatePkce(): Pkce {
  const verifier = randomBase64Url(32);
  return {
    verifier,
    challenge: challengeFor(verifier),
    state: randomBase64Url(16),
  };
}

export function matchesState(pkce: Pkce, returned: string): boolean {
  if (returned.length !== pkce.state.length) return false;
  let diff = 0;
  for (let i = 0; i < returned.length; i++) {
    diff |= returned.charCodeAt(i) ^ pkce.state.charCodeAt(i);
  }
  return diff === 0;
}

// BASE64URL(SHA256(verifier)), unpadded.
export function challengeFor(verifier: string): string {
  return createHash("sha256").update(verifier, "utf8").digest("base64url");
}

function randomBase64Url(bytes: number): string {
  return randomBytes(bytes).toString("base64url");
}
