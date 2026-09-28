// Port of crates/nori-gmail/src/oauth.rs — installed-app authorization code flow.
//
// Flow: bind loopback -> build auth URL -> open system browser ->
// await single callback -> exchange code. The secret never appears in the URL,
// PKCE binds the attempt, state ties the callback to it.

import { createServer, type Server } from "node:http";
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
}

interface TokenResponse {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  scope?: string;
  token_type?: string;
}

/**
 * A loopback listener, already bound, so the redirect URI is known before the
 * browser is ever launched.
 *
 * Port 0 lets the OS pick a free port. That is not a workaround: Google's
 * loopback redirect matching ignores the port, so a range of them is correct
 * and a fixed one would just invite collisions. Binding first is required,
 * because the port has to appear in the redirect URI the browser is sent to.
 */
export class LoopbackRedirect {
  private constructor(
    private readonly server: Server,
    readonly redirectUri: string,
  ) {}

  static async bind(): Promise<LoopbackRedirect> {
    const server = createServer();
    const port = await new Promise<number>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        const address = server.address();
        if (address == null || typeof address === "string") {
          reject(new Error("the loopback listener bound an unexpected address"));
          return;
        }
        resolve(address.port);
      });
    });
    return new LoopbackRedirect(server, `http://127.0.0.1:${port}`);
  }

  /**
   * Serve exactly one request, checking the callback against this attempt's
   * own state. Gives up after {@link CALLBACK_TIMEOUT_MS}: without a deadline
   * an abandoned sign-in leaves the process holding the socket forever.
   */
  awaitCallback(expectedState: string, onReady?: () => void): Promise<string> {
    const server = this.server;
    return new Promise<string>((resolve, reject) => {
      let settled = false;
      const finish = (fn: () => void) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        server.close();
        fn();
      };

      const timer = setTimeout(() => {
        finish(() =>
          reject(
            new Error(
              `timed out after ${CALLBACK_TIMEOUT_MS / 1000}s waiting for the browser to return`,
            ),
          ),
        );
      }, CALLBACK_TIMEOUT_MS);
      // The deadline is cleared when a request lands; without this it would
      // hold the process open for five minutes after a completed sign-in.
      timer.unref?.();

      // An abandoned sign-in must not keep the process alive on its own.
      server.unref();

      server.on("error", (err) => {
        finish(() =>
          reject(new Error(`the loopback listener on ${this.redirectUri} failed: ${err.message}`)),
        );
      });

      // The request line carries the whole query string, and one request is
      // served, so the connection closes immediately afterwards.
      server.on("request", (req, res) => {
        const target = req.url ?? "/";
        const query = target.includes("?") ? target.slice(target.indexOf("?") + 1) : "";
        let params: URLSearchParams;
        try {
          params = new URLSearchParams(query);
        } catch (err) {
          res.writeHead(400, { "content-type": "text/html; charset=utf-8" });
          res.end(FAILURE_PAGE);
          finish(() => reject(err instanceof Error ? err : new Error(String(err))));
          return;
        }
        try {
          const code = interpretCallback(params, expectedState);
          res.writeHead(200, {
            "content-type": "text/html; charset=utf-8",
            connection: "close",
          });
          res.end(SUCCESS_PAGE);
          finish(() => resolve(code));
        } catch (err) {
          res.writeHead(400, {
            "content-type": "text/html; charset=utf-8",
            connection: "close",
          });
          // The detail stays out of the page: this text is rendered in the
          // user's browser, and a token or a verifier must never reach it.
          res.end(FAILURE_PAGE);
          finish(() => reject(err instanceof Error ? err : new Error(String(err))));
        }
      });

      onReady?.();
    });
  }
}

const SUCCESS_PAGE =
  `<!doctype html><meta charset=utf-8><title>Nori</title>` +
  `<body style="font:16px system-ui;padding:3rem;max-width:32rem">` +
  `<h1>Nori is connected</h1><p>You can close this tab and go back to Nori.</p>`;

const FAILURE_PAGE =
  `<!doctype html><meta charset=utf-8><title>Nori</title>` +
  `<body style="font:16px system-ui;padding:3rem;max-width:32rem">` +
  `<h1>Sign-in did not complete</h1>` +
  `<p>You can close this tab. Nori will report the problem.</p>`;

/** Start a sign-in: bind the loopback, then build the URL that points at it. */
export async function beginAuth(
  credentials: Credentials,
): Promise<{ request: AuthorizationRequest; redirect: LoopbackRedirect }> {
  const redirect = await LoopbackRedirect.bind();
  const request = buildAuthUrl(credentials, redirect.redirectUri);
  return { request, redirect };
}

function buildAuthUrl(credentials: Credentials, redirectUri: string): AuthorizationRequest {
  const pkce: Pkce = generatePkce();
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
  return { url, redirectUri, verifier: pkce.verifier, state: pkce.state };
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
