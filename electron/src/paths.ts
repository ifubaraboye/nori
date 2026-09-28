import { existsSync } from "node:fs";
import { dirname, join } from "node:path";

/**
 * Locate the directory holding `electron/dist` and `web/dist`.
 *
 * The bundler replaces `__dirname` with the *build machine's* source path, so
 * the preload was looked up at `src/preload.cjs`, was never found, and
 * `window.nori` was never defined. The app then ran on sample mail with no
 * way to sign in, and nothing in the renderer could tell that apart from a
 * working build. Electron's app path is the anchor instead.
 *
 * That path is the directory of the entry file, so its depth varies with how
 * the app was started: the asar root when packaged, `electron/dist` for
 * `electron dist/main.cjs`, `electron/` for a script placed beside it. Rather
 * than assume a depth, walk up to the first ancestor that actually holds the
 * layout and let the filesystem decide.
 *
 * `has` is injectable so the walk can be tested without a real install.
 */
export function findResourceRoot(
  start: string,
  has: (path: string) => boolean = existsSync,
): string {
  let dir = start;
  // Bounded on purpose: the layout is at most a few levels above the entry
  // file, and an unbounded walk would climb to the filesystem root on a
  // broken install and report a plausible-looking answer.
  for (let depth = 0; depth < 5; depth += 1) {
    if (has(join(dir, "electron", "dist", "preload.cjs"))) return dir;
    const up = dirname(dir);
    if (up === dir) break;
    dir = up;
  }
  // Nothing matched. Return the start, so the caller reports a concrete
  // missing path against a real directory instead of guessing.
  return start;
}
