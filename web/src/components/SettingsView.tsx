import {
  SETTINGS_PAGES,
  getSetting,
  settingDescription,
  settingElementId,
  settingLabel,
  settingsPageIcon,
  settingsPageLabel,
  settingsPageNavId,
  type Setting,
  type SettingsPage,
  type SettingsState,
} from "../state/settings";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { ToggleSwitch } from "./ToggleSwitch";
import "./SettingsView.css";

export type SettingsAccountStatus =
  | "disconnected"
  | "connecting"
  | "fetching"
  | "connected"
  | "needsReauth"
  | "failed";

export interface SettingsAccount {
  status: SettingsAccountStatus;
  address?: string;
  mailCount?: number;
  labelCount?: number;
  /** Why the last sign-in failed. Shown verbatim on the failed page. */
  reason?: string;
  /** True while the browser is open waiting for consent. */
  waiting?: boolean;
}

interface SettingsViewProps {
  page: SettingsPage;
  settings: SettingsState;
  account: SettingsAccount;
  onPage: (page: SettingsPage) => void;
  onSet: (setting: Setting, enabled: boolean) => void;
  onSignIn: () => void;
  onSignOut: () => void;
}

function RowText({ title, description }: { title: string; description: string }) {
  return (
    <div className="nori-settings-row-text">
      <div className="nori-settings-row-title">{title}</div>
      <div className="nori-settings-row-description">{description}</div>
    </div>
  );
}

function NoteRow({
  title,
  description,
  isLast,
}: {
  title: string;
  description: string;
  isLast: boolean;
}) {
  return (
    <div
      className={
        isLast ? "nori-settings-row nori-settings-row--last" : "nori-settings-row"
      }
    >
      <RowText title={title} description={description} />
    </div>
  );
}

function ToggleRow({
  setting,
  settings,
  isLast,
  onSet,
}: {
  setting: Setting;
  settings: SettingsState;
  isLast: boolean;
  onSet: (setting: Setting, enabled: boolean) => void;
}) {
  const on = getSetting(settings, setting);
  return (
    <div
      className={
        isLast
          ? "nori-settings-row nori-settings-toggle-row nori-settings-row--last"
          : "nori-settings-row nori-settings-toggle-row"
      }
    >
      <div className="nori-settings-toggle-text">
        <RowText title={settingLabel(setting)} description={settingDescription(setting)} />
      </div>
      <ToggleSwitch
        id={settingElementId(setting)}
        label={settingLabel(setting)}
        on={on}
        onToggle={() => onSet(setting, !on)}
      />
    </div>
  );
}

function ValueRow({
  label,
  value,
  isLast,
}: {
  label: string;
  value: string;
  isLast: boolean;
}) {
  return (
    <div
      className={
        isLast ? "nori-settings-row nori-settings-value-row nori-settings-row--last" : "nori-settings-row nori-settings-value-row"
      }
    >
      <div className="nori-settings-value-key">{label}</div>
      <div className="nori-settings-value-text">{value}</div>
    </div>
  );
}

function AccountHeading({
  action,
  onSignIn,
  onSignOut,
}: {
  action?: { kind: "signin"; label: string } | { kind: "signout" };
  onSignIn: () => void;
  onSignOut: () => void;
}) {
  return (
    <div className="nori-settings-page-heading-row">
      <div className="nori-settings-page-heading nori-settings-page-heading--inline">Account</div>
      {action?.kind === "signin" && (
        <div id="account-sign-in-wrap" className="nori-settings-page-heading-action">
          <Button
            id="account-sign-in"
            label={action.label}
            buttonStyle="accent"
            onClick={onSignIn}
          />
        </div>
      )}
      {action?.kind === "signout" && (
        <div id="account-sign-out-wrap" className="nori-settings-page-heading-action">
          <Button
            id="account-sign-out"
            label="Disconnect"
            buttonStyle="subtle"
            onClick={onSignOut}
          />
        </div>
      )}
    </div>
  );
}

/** Port of views/settings_view.rs: page column + content column. */
export function SettingsView({
  page,
  settings,
  account,
  onPage,
  onSet,
  onSignIn,
  onSignOut,
}: SettingsViewProps) {
  return (
    <div id="settings" className="nori-settings">
      <div id="settings-nav" className="nori-settings-nav">
        <div id="settings-nav-list" className="nori-settings-nav-list">
          {SETTINGS_PAGES.map((entry) => {
            const selected = entry === page;
            return (
              <button
                key={entry}
                id={settingsPageNavId(entry)}
                type="button"
                role="button"
                aria-label={settingsPageLabel(entry)}
                aria-selected={selected}
                className={
                  selected ? "nori-settings-nav-row nori-settings-nav-row--selected" : "nori-settings-nav-row"
                }
                onClick={() => onPage(entry)}
              >
                <Icon
                  path={settingsPageIcon(entry)}
                  size={15}
                  color={selected ? "var(--nori-text)" : "var(--nori-faint)"}
                />
                <span>{settingsPageLabel(entry)}</span>
              </button>
            );
          })}
        </div>
      </div>
      <div id="settings-content" className="nori-settings-content">
        <div id="settings-content-scroll" className="nori-settings-content-scroll">
          <div className="nori-settings-page">
            {page !== "account" && (
              <div className="nori-settings-page-heading">{settingsPageLabel(page)}</div>
            )}
            {page === "general" && (
              <>
                <NoteRow
                  title="Local by default"
                  description="Nori keeps your mail on this machine. Nothing is uploaded, and this prototype ships with a bundled set of sample messages rather than a connected account."
                  isLast={false}
                />
                <ToggleRow setting="markReadOnOpen" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="unreadBadges" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="confirmBeforeArchive" settings={settings} isLast onSet={onSet} />
              </>
            )}
            {page === "appearance" && (
              <>
                <ToggleRow setting="lightMode" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="compactRows" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="showSender" settings={settings} isLast onSet={onSet} />
              </>
            )}
            {page === "mail" && (
              <>
                <ToggleRow setting="openInTab" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="groupConversations" settings={settings} isLast={false} onSet={onSet} />
                <ToggleRow setting="showAttachments" settings={settings} isLast onSet={onSet} />
              </>
            )}
            {page === "account" && (
              <AccountPage account={account} onSignIn={onSignIn} onSignOut={onSignOut} />
            )}
            {page === "about" && (
              <>
                <NoteRow
                  title="Nori"
                  description="A native mail client prototype. The sample mail, the settings above, and these facts are all part of the prototype."
                  isLast={false}
                />
                <ValueRow label="Version" value="0.1.0" isLast={false} />
                <ValueRow label="Interface" value="Electron + React (TypeScript)" isLast={false} />
                <ValueRow label="Source" value="web/, electron/" isLast />
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function AccountPage({
  account,
  onSignIn,
  onSignOut,
}: {
  account: SettingsAccount;
  onSignIn: () => void;
  onSignOut: () => void;
}) {
  switch (account.status) {
    case "disconnected":
      return (
        <>
          <AccountHeading
            action={{ kind: "signin", label: "Sign in with Gmail" }}
            onSignIn={onSignIn}
            onSignOut={onSignOut}
          />
          <NoteRow
            title="No account connected"
            description="Sign in to sync a Gmail mailbox. Nori reads mail on your own machine and sends nothing to any server of ours."
            isLast={false}
          />
          <ValueRow label="Address" value="Not connected" isLast={false} />
          <ValueRow label="Storage" value="Token file, created on first sign-in" isLast />
        </>
      );
    case "connecting":
      return (
        <>
          <AccountHeading onSignIn={onSignIn} onSignOut={onSignOut} />
          <NoteRow
            title="Waiting for Google…"
            description="Your browser should have opened Google's consent screen. Nori is listening on a local port for the redirect it sends back, then it will start fetching your mail. Nothing here can complete until you approve it there."
            isLast
          />
        </>
      );
    case "fetching":
      return (
        <>
          <AccountHeading onSignIn={onSignIn} onSignOut={onSignOut} />
          <NoteRow
            title="Fetching your mail…"
            description={`Signed in as ${account.address ?? ""}. Reading your mailbox now — this takes a minute on a large one, and your mail appears as it arrives.`}
            isLast
          />
        </>
      );
    case "connected":
      return (
        <>
          <AccountHeading
            action={{ kind: "signout" }}
            onSignIn={onSignIn}
            onSignOut={onSignOut}
          />
          <NoteRow
            title={account.address ?? ""}
            description="Synced. Mail and labels are read on demand, and read state and stars are written back."
            isLast={false}
          />
          <ValueRow
            label="Synced"
            value={`${account.mailCount ?? 0} messages, ${account.labelCount ?? 0} labels`}
            isLast={false}
          />
          <ValueRow label="Refresh" value="On open, and every few minutes" isLast />
        </>
      );
    case "needsReauth":
      return (
        <>
          <AccountHeading
            action={{ kind: "signin", label: "Sign in again" }}
            onSignIn={onSignIn}
            onSignOut={onSignOut}
          />
          <NoteRow
            title={`${account.address ?? ""} needs to sign in again`}
            description="Google expires the token after about a week while the app is unverified. Nothing was lost — signing in again picks up where it left off."
            isLast
          />
        </>
      );
    case "failed":
      return (
        <>
          <AccountHeading
            action={{ kind: "signin", label: "Try again" }}
            onSignIn={onSignIn}
            onSignOut={onSignOut}
          />
          <NoteRow
            title="Could not connect"
            description={account.reason ?? "Sign-in did not complete."}
            isLast
          />
        </>
      );
  }
}
