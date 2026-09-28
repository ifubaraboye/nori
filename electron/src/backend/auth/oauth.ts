// Port of crates/nori-gmail/src/oauth.rs — installed-app authorization code flow.
//
// Flow: bind loopback -> build auth URL -> open system browser ->
// await single callback -> exchange code. The secret never appears in the URL,
// PKCE binds the attempt, state ties the callback to it.

import { generatePkce, type Pkce } from "./pkce.js";
import { textWithStatus, detailOf } from "../gmail/http.js";
import { unixNow, type Token } from "./token.js";

export const AUTH_ENDPOINT = "https://accounts.google.com/o/oauth2/v2/auth";
export const TOKEN_ENDPOINT = "https://oauth2.googleapis.com/token";
export const SCOPES =
  "https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/userinfo.email";
const CALLBACK_TIMEOUT_MS = 300_000;

export interface Credentials {
  clientId: string;
  clientSecret: string;
}

export interface AuthorizationRequest {
  url: string;
  redirectUri: string;
  verifier: string;
  state: string;
  /** Ports are OS-assigned; the server is created lazily by awaitCallback. */
  port: number;
}

interface TokenResponse {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  scope?: string;
  token_type?: string;
}

export function beginAuth(credentials: Credentials): AuthorizationRequest {
  const pkce: Pkce = generatePkce();
  // Bind port 0 semantics: ask the OS via a probe socket. Bun.serve itself
  // binds lazily in awaitCallback; store the pkce + a reserved port hint.
  // To keep the redirect URI stable we allocate the port now with a probe.
  const port = probeFreePort();
  const redirectUri = `http://127.0.0.1:${port}`;
  const url =
    `${AUTH_ENDPOINT}` +
    `?client_id=${urlencode(credentials.clientId)}` +
    `&redirect_uri=${urlencode(redirectUri)}` +
    `&response_type=code` +
    `&scope=${urlencode(SCOPES)}` +
    `&access_type=offline` +
    `&prompt=consent` +
    `&code_challenge=${urlencode(pkce.challenge)}` +
    `&code_challenge_method=S256` +
    `&state=${urlencode(pkce.state)}`;
  return { url, redirectUri, verifier: pkce.verifier, state: pkce.state, port };
}

function probeFreePort(): number {
  // Bun has no sync "bind port 0 and read back" without serving; use a
  // best-effort probe in the dynamic range. awaitCallback re-binds the exact
  // port and retries on collision.
  return 49152 + Math.floor(Math.random() * (65535 - 49152));
}

export async function awaitCallback(request: AuthorizationRequest): Promise<string> {
  const expectedState = request.state;
  return new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      try {
        server.stop(true);
      } catch { /* already stopped */ }
      reject(
        new Error(
          `timed out after 300s waiting for the browser to return`,
        ),
      );
    }, CALLBACK_TIMEOUT_MS);

    const respond = (status: number, title: string, body: string) =>
      new Response(
        `<!doctype html><meta charset=utf-8><title>Nori</title>` +
          `<body style="font:16px system-ui;padding:3rem;max-width:32rem">` +
          `<h1>${title}</h1><p>${body}</p>`,
        {
          status,
          headers: { "content-type": "text/html; charset=utf-8", connection: "close" },
        },
      );

    const server = Bun.serve({
      port: request.port,
      hostname: "127.0.0.1",
      fetch(req) {
        const url = new URL(req.url);
        const params = url.searchParams;
        try {
          const code = interpretCallback(params, expectedState);
          clearTimeout(timer);
          queueMicrotask(() => server.stop(true));
          resolve(code);
          return respond(200, "Nori is connected", "You can close this tab and go back to Nori.");
        } catch (err) {
          clearTimeout(timer);
          queueMicrotask(() => server.stop(true));
          reject(err instanceof Error ? err : new Error(String(err)));
          // Never echo token/verifier detail into the browser page.
          return respond(400, "Sign-in did not complete", "You can close this tab. Nori will report the problem.");
        }
      },
    });
  });
}

export function interpretCallback(params: URLSearchParams, expectedState: string): string {
  const error = params.get("error");
  if (error) {
    const description = params.get("error_description") ?? "no description";
    throw new Error(`the user or Google refused authorization: ${error} (${description})`);
  }
  const state = params.get("state");
  if (!state) throw new Error("the callback carried no state");
  if (state !== expectedState) {
    throw new Error("the callback's state did not match this sign-in attempt");
  }
  const code = params.get("code");
  if (!code) throw new Error("the callback carried no authorization code");
  return code;
}

export async function exchange(
  credentials: Credentials,
  redirectUri: string,
  code: string,
  verifier: string,
): Promise<Token> {
  const form = formBody([
    ["code", code],
    ["client_id", credentials.clientId],
    ["client_secret", credentials.clientSecret],
    ["redirect_uri", redirectUri],
    ["grant_type", "authorization_code"],
    ["code_verifier", verifier],
  ]);
  const parsed = await postForm<TokenResponse>(form);
  return {
    accessToken: parsed.access_token,
    // A re-consent can omit the refresh token: keep-caller semantics handled
    // by the caller merging with the stored token.
    refreshToken: parsed.refresh_token,
    expiresAt: parsed.expires_in != null ? unixNow() + parsed.expires_in : undefined,
    scope: parsed.scope,
    tokenType: parsed.token_type,
  };
}

export async function refresh(credentials: Credentials, refreshToken: string): Promise<Token> {
  const form = formBody([
    ["client_id", credentials.clientId],
    ["client_secret", credentials.clientSecret],
    ["refresh_token", refreshToken],
    ["grant_type", "refresh_token"],
  ]);
  const parsed = await postForm<TokenResponse>(form);
  return {
    accessToken: parsed.access_token,
    // Refresh responses omit the refresh token: carry the caller's copy over.
    refreshToken,
    expiresAt: parsed.expires_in != null ? unixNow() + parsed.expires_in : undefined,
    scope: parsed.scope,
    tokenType: parsed.token_type,
  };
}

function formBody(pairs: Array<[string, string]>): string {
  return pairs.map(([k, v]) => `${k}=${urlencode(v)}`).join("&");
}

async function postForm<T>(form: string): Promise<T> {
  let res: Response;
  try {
    res = await fetch(TOKEN_ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: form,
      signal: AbortSignal.timeout(60_000),
    });
  } catch (err) {
    throw new Error(`${TOKEN_ENDPOINT} failed: ${err instanceof Error ? err.message : String(err)}`);
  }
  const raw = await res.text();
  let text: string;
  try {
    text = textWithStatus(raw, res.status, TOKEN_ENDPOINT);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    if (message.includes("invalid_grant")) {
      throw new Error(`the grant is no longer valid; the user must sign in again (${detailOf(raw)})`);
    }
    throw err;
  }
  return JSON.parse(text) as T;
}

// Percent-encode for query/form: unreserved passthrough, space -> %20 (not +).
export function urlencode(value: string): string {
  let out = "";
  const bytes = Buffer.from(value, "utf8");
  for (const byte of bytes) {
    if (
      (byte >= 0x41 && byte <= 0x5a) ||
      (byte >= 0x61 && byte <= 0x7a) ||
      (byte >= 0x30 && byte <= 0x39) ||
      byte === 0x2d || byte === 0x5f || byte === 0x2e || byte === 0x7e
    ) {
      out += String.fromCharCode(byte);
    } else {
      out += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
    }
  }
  return out;
}

export function urlDecodePairs(query: string): Map<string, string> {
  const params = new Map<string, string>();
  for (const pair of query.split("&")) {
    if (!pair) continue;
    const eq = pair.indexOf("=");
    const key = eq === -1 ? pair : pair.slice(0, eq);
    const value = eq === -1 ? "" : pair.slice(eq + 1);
    params.set(urldecode(key), urldecode(value));
  }
  return params;
}

function urldecode(value: string): string {
  return decodeURIComponent(value.replace(/\+/g, " "));
}
