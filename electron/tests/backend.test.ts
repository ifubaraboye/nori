import { describe, expect, test } from "bun:test";
import { challengeFor, generatePkce, matchesState } from "../src/backend/auth/pkce";
import { urlencode, interpretCallback, beginAuth } from "../src/backend/auth/oauth";
import { tokenFromExchange, isFresh, canRefresh, discoverCredentials, FileTokenStore, MigratingTokenStore, type Token } from "../src/backend/auth/token";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { textWithStatus, detailOf, isTransient } from "../src/backend/gmail/http";
import { parseRecipients, guessMime, buildSendRaw, MAX_SEND_BYTES } from "../src/backend/gmail/send";
import { folderCountsFromLookup } from "../src/backend/gmail/counts";
import { mayReplaceIndex, type MailIndex } from "../src/backend/gmail/cache";
import { sanitizeHtml } from "../src/backend/gmail/sanitize";
import { boundedHtml, fallbackText, hasRemoteImages } from "../src/backend/gmail/policy";
import { parseHtmlBody, plainText, textBlocks, inlineCidImages } from "../src/backend/gmail/rich";
import { findResourceRoot } from "../src/paths";

describe("token file compatibility", () => {
  const dir = mkdtempSync(join(tmpdir(), "nori-token-"));

  test("a snake_case token from the Rust build still loads", () => {
    // The Rust version wrote access_token / refresh_token / expires_at.
    // Reading only camelCase yields a token with no access token at all, which
    // then reads as "not signed in" and sends the user through a pointless
    // re-consent for a credential that was already on disk.
    const path = join(dir, "legacy.json");
    writeFileSync(
      path,
      JSON.stringify({
        access_token: "ya29.legacy",
        refresh_token: "1//legacy",
        expires_at: 1790615097,
        scope: "openid",
        token_type: "Bearer",
      }),
    );
    const token = new FileTokenStore(path).load();
    expect(token?.accessToken).toBe("ya29.legacy");
    expect(token?.refreshToken).toBe("1//legacy");
    expect(token?.expiresAt).toBe(1790615097);
    expect(token?.tokenType).toBe("Bearer");
    expect(canRefresh(token as Token)).toBe(true);
  });

  test("a camelCase token loads, and survives a round trip", () => {
    const path = join(dir, "modern.json");
    const store = new FileTokenStore(path);
    const token = tokenFromExchange("ya29.modern", "1//modern", 3600);
    store.save(token);
    const loaded = store.load();
    expect(loaded?.accessToken).toBe("ya29.modern");
    expect(loaded?.refreshToken).toBe("1//modern");
  });

  test("a token file with no access token is treated as absent", () => {
    const path = join(dir, "garbage.json");
    writeFileSync(path, JSON.stringify({ note: "not a token" }));
    expect(new FileTokenStore(path).load()).toBeNull();
  });

  test("a missing file is the first run, not a failure", () => {
    expect(new FileTokenStore(join(dir, "absent.json")).load()).toBeNull();
  });
});

describe("pkce (RFC 7636)", () => {
  test("s256 matches the RFC appendix B vector", () => {
    expect(challengeFor("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")).toBe(
      "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
    );
  });
  test("two attempts never share secrets", () => {
    const a = generatePkce();
    const b = generatePkce();
    expect(a.verifier).not.toBe(b.verifier);
    expect(a.state).not.toBe(b.state);
    expect(a.challenge).toBe(challengeFor(a.verifier));
  });
  test("state matching rejects anything not ours", () => {
    const pkce = generatePkce();
    expect(matchesState(pkce, pkce.state)).toBe(true);
    expect(matchesState(pkce, "")).toBe(false);
    expect(matchesState(pkce, `${pkce.state}x`)).toBe(false);
  });
});

describe("oauth url encoding", () => {
  test("space encodes as %20 and scopes survive", async () => {
    const creds = { clientId: "id.apps.googleusercontent.com", clientSecret: "secret" };
    const { request, redirect } = await beginAuth(creds);
    expect(request.url).toContain("code_challenge_method=S256");
    expect(request.url).toContain("access_type=offline");
    expect(request.url).toContain("gmail.modify%20https");
    // The secret belongs in the token exchange only, never in a URL that
    // lands in browser history.
    expect(request.url).not.toContain("secret");
    // The bound port has to appear in the redirect URI the browser is sent to.
    expect(request.redirectUri).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
    expect(request.url).toContain(urlencode(request.redirectUri));
    expect(request.url).toContain(request.state);
    expect(redirect.redirectUri).toBe(request.redirectUri);
  });
  test("callback interpret: ok, wrong state, refusal, missing code", () => {
    const ok = new URLSearchParams({ code: "abc", state: "xyz" });
    expect(interpretCallback(ok, "xyz")).toBe("abc");
    expect(() => interpretCallback(new URLSearchParams({ code: "abc", state: "other" }), "xyz")).toThrow();
    expect(() =>
      interpretCallback(new URLSearchParams({ error: "access_denied", error_description: "no" }), "xyz"),
    ).toThrow();
    expect(() => interpretCallback(new URLSearchParams({ state: "xyz" }), "xyz")).toThrow();
  });

  test("the loopback serves one redirect and never echoes a secret", async () => {
    const creds = { clientId: "id", clientSecret: "GOCSPX-super-secret" };
    const { request, redirect } = await beginAuth(creds);
    const waiting = redirect.awaitCallback(request.state);

    const port = new URL(request.redirectUri).port;
    const ok = await fetch(`http://127.0.0.1:${port}/?code=the-code&state=${request.state}`);
    expect(ok.status).toBe(200);
    const body = await ok.text();
    expect(body).toContain("Nori is connected");
    // The page is rendered in a browser: a token or verifier must never
    // reach it.
    expect(body).not.toContain("the-code");
    expect(body).not.toContain(request.verifier);
    expect(body).not.toContain(request.state);
    expect(await waiting).toBe("the-code");
  });

  test("the checked-in .env at the repository root is found", () => {
    // Electron runs with its working directory at electron/, while the .env
    // sits at the repository root one level up. Only the working directory
    // was searched, so the credentials were reported missing in the one place
    // they are actually used.
    const root = mkdtempSync(join(tmpdir(), "nori-env-"));
    const appDir = join(root, "electron");
    mkdirSync(appDir);
    writeFileSync(
      join(root, ".env"),
      "NORI_GMAIL_CLIENT_ID=id.apps.googleusercontent.com\nNORI_GMAIL_CLIENT_SECRET=GOCSPX-secret\n",
    );
    // The app's own .env is absent, as it is in a checkout.
    expect(existsSync(join(appDir, ".env"))).toBe(false);
    const creds = discoverCredentials(appDir);
    expect(creds.clientId).toBe("id.apps.googleusercontent.com");
    expect(creds.clientSecret).toBe("GOCSPX-secret");
  });

  test("a token in the legacy location is found and copied forward", () => {
    // The Rust build wrote to ~/.config/nori; this port writes to Electron's
    // userData. Reading only the new path reported "not signed in" for an
    // account whose credential was already on disk.
    const dir = mkdtempSync(join(tmpdir(), "nori-tok-"));
    const current = new FileTokenStore(join(dir, "current", "a.token.json"));
    const legacy = new FileTokenStore(join(dir, "legacy", "a.token.json"));
    const token: Token = {
      accessToken: "ya29.token",
      refreshToken: "1//refresh",
      expiresAt: 4_000_000_000,
      scope: "gmail.modify",
      tokenType: "Bearer",
    };
    legacy.save(token);
    expect(current.load()).toBeNull();

    const store = new MigratingTokenStore(current, legacy);
    expect(store.load()?.accessToken).toBe("ya29.token");
    // Copied forward, so the fallback is only paid once.
    expect(current.load()?.accessToken).toBe("ya29.token");
    // And the current location wins once it has one.
    const fresh: Token = { ...token, accessToken: "ya29.newer" };
    store.save(fresh);
    expect(store.load()?.accessToken).toBe("ya29.newer");
  });

  test("a mismatched state is refused with a 400 and no echo", async () => {
    const creds = { clientId: "id", clientSecret: "s" };
    const { request, redirect } = await beginAuth(creds);
    // The handler is attached before the request is made, so the rejection is
    // never momentarily unhandled — Bun aborts the test on that.
    const outcome = redirect.awaitCallback(request.state).then(
      () => "resolved",
      (err: Error) => err.message,
    );
    const port = new URL(request.redirectUri).port;

    const res = await fetch(`http://127.0.0.1:${port}/?code=abc&state=wrong`);
    expect(res.status).toBe(400);
    const body = await res.text();
    expect(body).toContain("Sign-in did not complete");
    expect(body).not.toContain("abc");
    expect(await outcome).toMatch(/state/);
  });

  test("a refusal from Google is surfaced, not swallowed", async () => {
    const creds = { clientId: "id", clientSecret: "s" };
    const { request, redirect } = await beginAuth(creds);
    const outcome = redirect.awaitCallback(request.state).then(
      () => "resolved",
      (err: Error) => err.message,
    );
    const port = new URL(request.redirectUri).port;

    const res = await fetch(
      `http://127.0.0.1:${port}/?error=access_denied&error_description=The%20user%20said%20no&state=${request.state}`,
    );
    expect(res.status).toBe(400);
    expect(await outcome).toMatch(/access_denied/);
  });

  test("the listener is bound before the url is built, on a real port", async () => {
    const { request } = await beginAuth({ clientId: "id", clientSecret: "s" });
    // Port 0 would never be reachable, which is the whole reason bind() runs
    // before the URL is assembled.
    const port = Number(new URL(request.redirectUri).port);
    expect(port).toBeGreaterThan(0);
  });
});

describe("tokens", () => {
  test("freshness leaves a renewal margin; absent expiry is stale", () => {
    expect(isFresh(tokenFromExchange("a", "r", 3600))).toBe(true);
    expect(isFresh(tokenFromExchange("a", "r", 30))).toBe(false);
    expect(isFresh(tokenFromExchange("a", "r", undefined))).toBe(false);
  });
  test("refresh requires a refresh token", () => {
    expect(canRefresh(tokenFromExchange("a", undefined, 3600))).toBe(false);
    expect(canRefresh(tokenFromExchange("a", "r", 60))).toBe(true);
  });
});

describe("http errors", () => {
  test("401 is unauthorized and not transient", () => {
    try {
      textWithStatus('{"error":{"message":"Invalid Credentials"}}', 401, "u");
      expect.unreachable();
    } catch (err) {
      expect((err as Error).message).toContain("sign in again");
      expect(isTransient((err as { error: Parameters<typeof isTransient>[0] }).error)).toBe(false);
    }
  });
  test("403 quota is rate-limited and transient; permission is not", () => {
    const quota =
      '{"error":{"code":403,"message":"Quota exceeded for quota metric Units per minute per user"}}';
    try {
      textWithStatus(quota, 403, "u");
      expect.unreachable();
    } catch (err) {
      const e = (err as { error: Parameters<typeof isTransient>[0] }).error;
      expect(e.kind).toBe("rateLimited");
      expect(isTransient(e)).toBe(true);
    }
    try {
      textWithStatus('{"error":{"code":403,"message":"Insufficient Permission"}}', 403, "u");
      expect.unreachable();
    } catch (err) {
      const e = (err as { error: Parameters<typeof isTransient>[0] }).error;
      expect(e.kind).not.toBe("rateLimited");
    }
  });
  test("detail unwraps both Gmail error shapes", () => {
    expect(detailOf('{"error":{"message":"Not found"}}')).toBe("Not found");
    expect(detailOf('{"error":"invalid_grant","error_description":"revoked"}')).toContain("invalid_grant");
    expect(detailOf("<html>502</html>")).toBe("<html>502</html>");
  });
});

describe("send", () => {
  test("recipients split on comma/semicolon and validate", () => {
    expect(parseRecipients("a@example.com; b@example.com")).toEqual(["a@example.com", "b@example.com"]);
    expect(() => parseRecipients("not-an-address")).toThrow();
    expect(() => parseRecipients("  ")).toThrow();
  });
  test("mime guessing", () => {
    expect(guessMime("a.pdf")).toBe("application/pdf");
    expect(guessMime("a.JPG")).toBe("image/jpeg");
    expect(guessMime("noext")).toBe("application/octet-stream");
  });
  test("raw message round-trips through base64url", () => {
    const raw = buildSendRaw(["a@example.com"], "Hello", "body", []);
    const decoded = Buffer.from(raw, "base64url").toString("utf8");
    expect(decoded).toContain("To: a@example.com");
    expect(decoded).toContain("Subject: Hello");
    expect(MAX_SEND_BYTES).toBe(25 * 1024 * 1024);
  });
});

describe("counts", () => {
  test("archive is the residual; trash includes spam", () => {
    const counts = folderCountsFromLookup((id) => {
      const table: Record<string, [number, number]> = {
        INBOX: [10, 3],
        SENT: [4, 0],
        DRAFT: [1, 0],
        TRASH: [2, 0],
        SPAM: [1, 0],
        STARRED: [5, 0],
      };
      return table[id];
    }, 30);
    expect(counts.trash).toBe(3);
    expect(counts.archive).toBe(30 - 10 - 4 - 1 - 3);
    expect(counts.inboxUnread).toBe(3);
  });
});

describe("index cache veto", () => {
  const indexWith = (mails: number): MailIndex => ({
    account: "a",
    emails: Array.from({ length: mails }, (_, i) => ({
      id: String(i),
      sender: "s",
      address: "a",
      recipients: [],
      subject: "s",
      preview: "p",
      timestamp: "t",
      fullDate: "f",
      mailbox: "inbox",
      unread: false,
      starred: false,
    })),
    labels: [],
    assignments: [],
  });
  test("a thin store must not clobber a fuller file", () => {
    expect(mayReplaceIndex(indexWith(80), 1)).toBe(false);
    expect(mayReplaceIndex(indexWith(80), 0)).toBe(false);
    expect(mayReplaceIndex(indexWith(80), 80)).toBe(true);
    expect(mayReplaceIndex(indexWith(80), 83)).toBe(true);
  });
});

describe("sanitize + policy", () => {
  test("scripts are stripped, layout kept", () => {
    const clean = sanitizeHtml('<div><script>alert(1)</script><p>Hello</p><img src="cid:1"></div>');
    expect(clean).not.toContain("<script>");
    expect(clean).toContain("Hello");
    expect(clean).toContain("cid:1");
  });
  test("remote detection only counts img/css, oversized truncates", () => {
    expect(hasRemoteImages('<img src="https://example.com/a.png">')).toBe(true);
    expect(hasRemoteImages('<a href="https://example.com">x</a>')).toBe(false);
    expect(hasRemoteImages('<div style="background:url(http://x/y.png)">t</div>')).toBe(true);
    expect(fallbackText("<p>hi</p><p>there</p>")).toContain("hi");
    const { notice } = boundedHtml("x".repeat(25 * 1024 * 1024));
    expect(notice).toContain("size limit");
  });
});

describe("rich body", () => {
  test("headings, lists, quotes and trackers", () => {
    const blocks = parseHtmlBody("<h1>Title</h1><ul><li>a</li><li>b</li></ul><blockquote><p>q</p></blockquote>");
    expect(blocks[0]).toMatchObject({ kind: "heading" });
    expect(blocks[1]).toMatchObject({ kind: "list" });
    expect(blocks[2]).toMatchObject({ kind: "quote" });
    expect(plainText(blocks)).toContain("Title");
    expect(plainText(textBlocks(["a", "b"]))).toContain("a");
    const withTracker = parseHtmlBody('<img src="https://x/y.png" width="1" height="1"><p>t</p>');
    expect(withTracker.some((b) => b.kind === "image")).toBe(false);
  });
  test("cid images inline within budget", () => {
    const bytes = new Uint8Array([1, 2, 3]);
    const html = '<img src="cid:photo@x">';
    const out = inlineCidImages(html, [["<photo@x>", "image/png", bytes]]);
    expect(out).toContain("data:image/png;base64,");
  });
});

describe("finding the app's own files", () => {
  // The preload holds window.nori, and without it the app silently runs on
  // sample mail: no sign-in, no real mail, and nothing in the renderer to say
  // why. The bug was a lookup pointed at src/preload.cjs, so the depth the
  // app path arrives at is the thing worth pinning.
  const layout = "/repo";
  const has = (path: string) => path === "/repo/electron/dist/preload.cjs";

  test("walks up from the entry file's directory", () => {
    // `electron dist/main.cjs` makes the app path electron/dist.
    expect(findResourceRoot("/repo/electron/dist", has)).toBe(layout);
    // A script placed beside dist/ makes it electron/.
    expect(findResourceRoot("/repo/electron", has)).toBe(layout);
    // Packaged, the asar root already holds the layout.
    expect(findResourceRoot("/repo", has)).toBe(layout);
  });

  test("returns the start when nothing holds the layout", () => {
    // A plausible-looking wrong answer here is worse than none: the caller
    // reports a concrete missing path against the start instead.
    expect(findResourceRoot("/elsewhere", () => false)).toBe("/elsewhere");
  });

  test("stops at the filesystem root rather than looping", () => {
    // An unbounded walk would climb forever on a broken install.
    expect(findResourceRoot("/", () => false)).toBe("/");
  });
});
