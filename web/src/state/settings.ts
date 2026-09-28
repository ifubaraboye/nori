// Port of crates/nori-ui/src/model/settings.rs (types, labels, defaults).
// Persistence lives in useSettings: Electron host when present,
// localStorage fallback for standalone web.

export type SettingsPage = "general" | "appearance" | "mail" | "account" | "about";

export const SETTINGS_PAGES: SettingsPage[] = ["general", "appearance", "mail", "account", "about"];

export function settingsPageLabel(page: SettingsPage): string {
  switch (page) {
    case "general":
      return "General";
    case "appearance":
      return "Appearance";
    case "mail":
      return "Mail";
    case "account":
      return "Account";
    case "about":
      return "About";
  }
}

export function settingsPageIcon(page: SettingsPage): string {
  switch (page) {
    case "general":
      return "icons/settings.svg";
    case "appearance":
      return "icons/appearance.svg";
    case "mail":
      return "icons/mail.svg";
    case "account":
      return "icons/user.svg";
    case "about":
      return "icons/info.svg";
  }
}

export function settingsPageNavId(page: SettingsPage): string {
  return `settings-nav-page-${page}`;
}

export type Setting =
  | "markReadOnOpen"
  | "unreadBadges"
  | "confirmBeforeArchive"
  | "compactRows"
  | "showSender"
  | "lightMode"
  | "openInTab"
  | "groupConversations"
  | "showAttachments";

export function settingLabel(setting: Setting): string {
  switch (setting) {
    case "markReadOnOpen":
      return "Mark as read on open";
    case "unreadBadges":
      return "Unread count badges";
    case "confirmBeforeArchive":
      return "Confirm before archiving";
    case "compactRows":
      return "Tighten the mail list to one line per message";
    case "showSender":
      return "Show sender in message view";
    case "lightMode":
      return "Light mode";
    case "openInTab":
      return "Open messages in a tab";
    case "groupConversations":
      return "Group conversations";
    case "showAttachments":
      return "Show attachments inline";
  }
}

export function settingElementId(setting: Setting): string {
  switch (setting) {
    case "markReadOnOpen":
      return "settings-toggle-mark-read-on-open";
    case "unreadBadges":
      return "settings-toggle-unread-badges";
    case "confirmBeforeArchive":
      return "settings-toggle-confirm-before-archive";
    case "compactRows":
      return "settings-toggle-compact-rows";
    case "showSender":
      return "settings-toggle-show-sender";
    case "lightMode":
      return "settings-toggle-light-mode";
    case "openInTab":
      return "settings-toggle-open-in-tab";
    case "groupConversations":
      return "settings-toggle-group-conversations";
    case "showAttachments":
      return "settings-toggle-show-attachments";
  }
}

export function settingDescription(setting: Setting): string {
  switch (setting) {
    case "markReadOnOpen":
      return "Clear the unread dot as soon as a message is opened, the way most clients do.";
    case "unreadBadges":
      return "Show unread totals next to each mailbox in the sidebar.";
    case "confirmBeforeArchive":
      return "Ask before moving messages out of the current mailbox.";
    case "compactRows":
      return (
        "Show sender, label, subject and preview on one line instead of three, so " +
        "roughly twice as many messages fit on screen."
      );
    case "showSender":
      return "Keep the sender line visible above the message body.";
    case "lightMode":
      return "Use the light palette. The switch takes effect immediately and is remembered for next time.";
    case "openInTab":
      return "Keep a tab for every opened message so you can jump back.";
    case "groupConversations":
      return "Thread replies together under the most recent message.";
    case "showAttachments":
      return "Render attachment chips inline instead of a footer list.";
  }
}

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

export function getSetting(state: SettingsState, setting: Setting): boolean {
  return state[setting];
}
