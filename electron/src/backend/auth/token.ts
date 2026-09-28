// Port of crates/nori-gmail/src/token.rs + config.rs (credentials discovery).
import { mkdirSync, readFileSync, writeFileSync, rmSync, existsSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import type { Credentials } from "./oauth.js";

export interface Token {
  accessToken: string;
  refreshToken?: string;
  /** Unix seconds. Absent = already stale, never eternal. */
  expiresAt?: number;
  scope?: string;
  tokenType?: string;
}

export function tokenFromExchange(
  accessToken: string,
  refreshToken: string | undefined,
  expiresIn: number | undefined,
): Token {
  return {
    accessToken,
    refreshToken,
    expiresAt: expiresIn != null ? unixNow() + expiresIn : undefined,
  };
}

export function unixNow(): number {
  return Math.floor(Date.now() / 1000);
}

// 60s renewal margin: a token valid at sync start can lapse mid-flight.
export function isFresh(token: Token): boolean {
  return token.expiresAt != null && token.expiresAt > unixNow() + 60;
}

export function canRefresh(token: Token): boolean {
  return token.refreshToken != null;
}

function configBase(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  if (xdg) return xdg;
  return join(homedir(), ".config");
}

export function tokenPathForAccount(account: string, base = configBase()): string {
  return join(base, "nori", `${account}.token.json`);
}

export interface TokenStore {
  load(): Token | null;
  save(token: Token): void;
  clear(): void;
}

/**
 * Reads from the current location, falling back to the one the Rust build
 * used, and writes only to the current one.
 *
 * The fallback matters more than it looks: without it a token written by the
 * previous build reads as "not signed in", which sends the user through a
 * pointless re-consent for a credential that was already on disk. A token
 * found only in the old location is copied forward, so the migration happens
 * once rather than on every start.
 */
export class MigratingTokenStore implements TokenStore {
  constructor(
    private readonly current: FileTokenStore,
    private readonly legacy: FileTokenStore,
  ) {}

  load(): Token | null {
    const token = this.current.load();
    if (token) return token;
    const old = this.legacy.load();
    if (!old) return null;
    try {
      this.current.save(old);
    } catch {
      // A read-only userData is survivable: the legacy copy still works.
    }
    return old;
  }

  save(token: Token): void {
    this.current.save(token);
  }

  clear(): void {
    this.current.clear();
    this.legacy.clear();
  }
}

export class FileTokenStore implements TokenStore {
  constructor(readonly path: string) {}

  static withAccount(account: string): FileTokenStore {
    return new FileTokenStore(tokenPathForAccount(account));
  }

  /**
   * Read a token written by either build.
   *
   * The Rust version serialised `access_token` / `refresh_token` /
   * `expires_at` in snake_case; this port uses camelCase. Accepting both is
   * what lets a token file written before the port survive it — reading only
   * camelCase would silently yield a token with no access token at all, which
   * then reads as "not signed in" and sends the user through a pointless
   * re-consent for a credential that was sitting on disk the whole time.
   */
  load(): Token | null {
    try {
      const raw = JSON.parse(readFileSync(this.path, "utf8")) as Record<string, unknown>;
      const accessToken = (raw.accessToken ?? raw.access_token) as string | undefined;
      if (typeof accessToken !== "string" || accessToken === "") return null;
      const refreshToken = (raw.refreshToken ?? raw.refresh_token) as string | undefined;
      const expiresAt = (raw.expiresAt ?? raw.expires_at) as number | undefined;
      return {
        accessToken,
        refreshToken: typeof refreshToken === "string" ? refreshToken : undefined,
        expiresAt: typeof expiresAt === "number" ? expiresAt : undefined,
        scope: (raw.scope as string | undefined) ?? undefined,
        tokenType: (raw.tokenType ?? raw.token_type) as string | undefined,
      };
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") return null;
      throw err;
    }
  }

  save(token: Token): void {
    mkdirSync(dirname(this.path), { recursive: true, mode: 0o700 });
    writeFileSync(this.path, JSON.stringify(token, null, 2), { mode: 0o600 });
    try {
      chmodSync(this.path, 0o600);
    } catch { /* non-POSIX: ACLs govern */ }
  }

  clear(): void {
    try {
      rmSync(this.path, { force: true });
    } catch { /* idempotent */ }
  }
}

export class LastAccount {
  constructor(readonly path: string) {}

  static withConfigDir(): LastAccount {
    return new LastAccount(join(configBase(), "nori", "last-account"));
  }

  load(): string | null {
    try {
      const address = readFileSync(this.path, "utf8").trim();
      return address ? address : null;
    } catch {
      return null;
    }
  }

  save(address: string): void {
    mkdirSync(dirname(this.path), { recursive: true });
    writeFileSync(this.path, address);
  }

  clear(): void {
    try {
      rmSync(this.path, { force: true });
    } catch { /* idempotent */ }
  }
}

// Credentials discovery: env first, then $NORI_ENV_FILE / .env / $config/nori/.env.
export function credentialsFromEnv(): Credentials {
  const clientId = process.env.NORI_GMAIL_CLIENT_ID;
  const clientSecret = process.env.NORI_GMAIL_CLIENT_SECRET;
  if (!clientId) throw new Error("NORI_GMAIL_CLIENT_ID is not set");
  if (!clientSecret) throw new Error("NORI_GMAIL_CLIENT_SECRET is not set");
  return { clientId, clientSecret };
}

export function discoverCredentials(cwd = process.cwd()): Credentials {
  try {
    return credentialsFromEnv();
  } catch { /* fall through to files */ }
  const candidates: string[] = [];
  if (process.env.NORI_ENV_FILE) candidates.push(process.env.NORI_ENV_FILE);
  candidates.push(join(cwd, ".env"));
  // The app is started from electron/ but the checked-in .env lives at the
  // repository root, one level up. Looking only in the working directory found
  // it when scripts ran from the root and missed it when Electron ran, which
  // made the credentials look absent in the one place they are actually used.
  candidates.push(join(cwd, "..", ".env"));
  candidates.push(join(configBase(), "nori", ".env"));
  const searched: string[] = [];
  for (const file of candidates) {
    searched.push(file);
    if (!existsSync(file)) continue;
    const parsed = parseEnvFile(readFileSync(file, "utf8"));
    if (parsed) return parsed;
  }
  throw new Error(
    `no Gmail client credentials. Looked in env and in:\n${searched.join("\n")}\nCopy .env.example to .env.`,
  );
}

function parseEnvFile(contents: string): Credentials | null {
  let clientId = "";
  let clientSecret = "";
  for (const line of contents.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const body = trimmed.startsWith("export ") ? trimmed.slice(7).trimStart() : trimmed;
    const eq = body.indexOf("=");
    if (eq === -1) continue;
    const key = body.slice(0, eq).trim();
    const value = unquote(body.slice(eq + 1).trim());
    if (key === "NORI_GMAIL_CLIENT_ID") clientId = value;
    if (key === "NORI_GMAIL_CLIENT_SECRET") clientSecret = value;
  }
  return clientId && clientSecret ? { clientId, clientSecret } : null;
}

function unquote(value: string): string {
  if (value.length >= 2) {
    const first = value[0];
    const last = value[value.length - 1];
    if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
      return value.slice(1, -1);
    }
  }
  return value;
}

export function missingHint(): string {
  return "Set NORI_GMAIL_CLIENT_ID in env or .env. See .env.example.";
}
