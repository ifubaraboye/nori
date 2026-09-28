// Port of nori-ui model/settings.rs — app settings in $config/nori/settings.json.
import { mkdirSync, readFileSync, writeFileSync, rmSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";

export interface SettingsState {
  markReadOnOpen: boolean;
  unreadBadges: boolean;
  confirmBeforeArchive: boolean;
  compactRows: boolean;
  showSender: boolean;
  lightMode: boolean;
  openInTab: boolean;
  groupConversations: boolean;
  showAttachments: boolean;
}

export function defaultSettings(): SettingsState {
  return {
    markReadOnOpen: true,
    unreadBadges: true,
    confirmBeforeArchive: false,
    compactRows: true,
    showSender: true,
    lightMode: false,
    openInTab: true,
    groupConversations: false,
    showAttachments: false,
  };
}

export class SettingsStore {
  constructor(readonly path: string) {}

  static withConfigDir(): SettingsStore {
    const base = process.env.XDG_CONFIG_HOME ?? join(homedir(), ".config");
    return new SettingsStore(join(base, "nori", "settings.json"));
  }

  load(): SettingsState | null {
    try {
      const contents = readFileSync(this.path, "utf8");
      return { ...defaultSettings(), ...(JSON.parse(contents) as Partial<SettingsState>) };
    } catch {
      return null;
    }
  }

  save(settings: SettingsState): void {
    const parent = dirname(this.path);
    if (parent) mkdirSync(parent, { recursive: true });
    const temporary = `${this.path}.tmp`;
    writeFileSync(temporary, JSON.stringify(settings, null, 2));
    try {
      renameSync(temporary, this.path);
    } catch {
      writeFileSync(this.path, JSON.stringify(settings, null, 2));
      try {
        rmSync(temporary, { force: true });
      } catch { /* ignore */ }
    }
  }
}
