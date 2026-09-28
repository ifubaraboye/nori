// Port of crates/nori-gmail/src/http.rs — shared Gmail error type + status mapping.
export type GmailError =
  | { kind: "unauthorized" }
  | { kind: "rateLimited"; detail: string }
  | { kind: "historyExpired" }
  | { kind: "unreachable"; endpoint: string; reason: string }
  | { kind: "api"; endpoint: string; status: number; detail: string }
  | { kind: "malformed"; endpoint: string; reason: string };

export function gmailErrorMessage(err: GmailError): string {
  switch (err.kind) {
    case "unauthorized":
      return "the grant is no longer valid, sign in again";
    case "rateLimited":
      return `Gmail's rate limit was reached (${err.detail})`;
    case "historyExpired":
      return "the stored history id has aged out";
    case "unreachable":
      return `${err.endpoint} could not be reached: ${err.reason}`;
    case "api":
      return `${err.endpoint} returned ${err.status}: ${err.detail}`;
    case "malformed":
      return `could not read a response from ${err.endpoint}: ${err.reason}`;
  }
}

export function isTransient(err: GmailError): boolean {
  switch (err.kind) {
    case "unauthorized":
    case "historyExpired":
    case "malformed":
      return false;
    case "rateLimited":
    case "unreachable":
      return true;
    case "api":
      return err.status >= 500;
  }
}

export class GmailHttpError extends Error {
  constructor(readonly error: GmailError) {
    super(gmailErrorMessage(error));
    this.name = "GmailHttpError";
  }
}

export const MAX_BODY_BYTES = 32 * 1024 * 1024;

export async function gmailFetch(
  url: string,
  token: string,
  init: RequestInit = {},
): Promise<string> {
  let res: Response;
  try {
    res = await fetch(url, {
      ...init,
      headers: {
        ...(init.headers ?? {}),
        authorization: `Bearer ${token}`,
      },
      signal: init.signal ?? AbortSignal.timeout(60_000),
    });
  } catch (err) {
    throw new GmailHttpError({
      kind: "unreachable",
      endpoint: url,
      reason: err instanceof Error ? err.message : String(err),
    });
  }
  const raw = await res.text();
  return textWithStatus(raw.length > MAX_BODY_BYTES ? raw.slice(0, MAX_BODY_BYTES) : raw, res.status, url);
}

// Pair a body with its status. 401 -> unauthorized; 403/429 consult the body
// so a quota refusal never reads as a dead token.
export function textWithStatus(body: string, status: number, endpoint: string): string {
  if (status >= 200 && status < 300) return body;
  if (status === 401) {
    throw new GmailHttpError({ kind: "unauthorized" });
  }
  if (status === 403 || status === 429) {
    const detail = detailOf(body);
    if (isRateLimit(detail)) throw new GmailHttpError({ kind: "rateLimited", detail });
    if (status === 429) throw new GmailHttpError({ kind: "rateLimited", detail });
  }
  throw new GmailHttpError({ kind: "api", endpoint, status, detail: detailOf(body) });
}

export function detailOf(body: string): string {
  try {
    const flat = JSON.parse(body) as { error?: unknown; error_description?: unknown };
    if (typeof flat.error === "string") {
      return typeof flat.error_description === "string"
        ? `${flat.error}: ${flat.error_description}`
        : flat.error;
    }
    const wrapped = flat as { error?: { message?: unknown; description?: unknown } };
    if (wrapped.error && typeof wrapped.error === "object") {
      const message = wrapped.error.message;
      const description = wrapped.error.description;
      if (typeof message === "string") return message;
      if (typeof description === "string") return description;
    }
  } catch { /* not JSON: fall through */ }
  return body;
}

function isRateLimit(detail: string): boolean {
  const lower = detail.toLowerCase();
  return (
    lower.includes("quota exceeded") ||
    lower.includes("ratelimitexceeded") ||
    lower.includes("rate limit") ||
    lower.includes("quota metric")
  );
}
