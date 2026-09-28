import { describe, expect, test } from "bun:test";
import { challengeFor, generatePkce, matchesState } from "../src/backend/auth/pkce";
import { urlencode, interpretCallback, beginAuth } from "../src/backend/auth/oauth";
import { tokenFromExchange, isFresh, canRefresh } from "../src/backend/auth/token";
import { textWithStatus, detailOf, isTransient } from "../src/backend/gmail/http";
import { parseRecipients, guessMime, buildSendRaw, MAX_SEND_BYTES } from "../src/backend/gmail/send";
import { folderCountsFromLookup } from "../src/backend/gmail/counts";
import { mayReplaceIndex, type MailIndex } from "../src/backend/gmail/cache";
import { sanitizeHtml } from "../src/backend/gmail/sanitize";
import { boundedHtml, fallbackText, hasRemoteImages } from "../src/backend/gmail/policy";
import { parseHtmlBody, plainText, textBlocks, inlineCidImages } from "../src/backend/gmail/rich";

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
  test("space encodes as %20 and scopes survive", () => {
    const creds = { clientId: "id.apps.googleusercontent.com", clientSecret: "secret" };
    const request = beginAuth(creds);
    expect(request.url).toContain("code_challenge_method=S256");
    expect(request.url).toContain("access_type=offline");
    expect(request.url).toContain("gmail.modify%20https");
    expect(request.url).not.toContain("secret");
    expect(urlencode("a b+c~")).toBe("a%20b%2Bc~");
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
