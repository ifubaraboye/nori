import { useCallback, useEffect, useRef, useState } from "react";
import { replySeed, useMailStore, type MailAction } from "../state/store";
import { useSettings } from "../state/useSettings";
import {
  SETTINGS_PAGES,
  settingsPageLabel,
  type SettingsPage,
} from "../state/settings";
import { getNoriBridge } from "../bridge/noriBridge";
import { notifyOpen, notifyToggleStar, useNoriBackend } from "../bridge/backend";
import { mailboxLabel } from "../types/mail";
import { clampSidebarWidth, SIDEBAR_DEFAULT_WIDTH, Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { EmailList } from "./EmailList";
import { EmailTabs } from "./EmailTabs";
import { EmailView } from "./EmailView";
import { ComposeDialog } from "./ComposeDialog";
import { SearchDialog } from "./SearchDialog";
import { SettingsView, type SettingsAccount } from "./SettingsView";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./MailApp.css";

/**
 * Port of views/mail_app.rs Render.
 * Keyboard map mirrors actions::register_key_bindings in actions.rs,
 * plus settings (Ctrl+Shift+P toggle, Up/Down move between pages).
 */
export function MailApp() {
  const { state, dispatch, summaries, activeEmail, counts } = useMailStore();
  const [settings, setSettings] = useSettings();
  useNoriBackend();
  // Host menu "Toggle Sidebar" arrives as a push event under Electron.
  useEffect(() => {
    const bridge = getNoriBridge();
    if (!bridge) return;
    return bridge.subscribe((event) => {
      if (event.type === "toggle-sidebar") setSidebarVisible((v) => !v);
    });
  }, []);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [sidebarWidth, setSidebarWidth] = useState(SIDEBAR_DEFAULT_WIDTH);
  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false);
  const [accountAddress, setAccountAddress] = useState<string | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const resizeRef = useRef<{ startX: number; startWidth: number } | null>(null);

  // The connected account, read live like the Rust settings view does.
  useEffect(() => {
    const bridge = getNoriBridge();
    if (!bridge?.sync) return;
    bridge
      .sync()
      .then(({ account }) => setAccountAddress(account))
      .catch(() => undefined);
  }, []);

  const closeOverlay = useCallback(() => dispatch({ type: "close-overlay" }), [dispatch]);

  const openComposeDefault = useCallback(
    () => dispatch({ type: "open-compose", seed: { to: "", subject: "", body: "" } }),
    [dispatch],
  );

  const openEmail = useCallback(
    (id: number, extra?: { fromSearch?: boolean }) => {
      const action: MailAction = extra?.fromSearch
        ? {
            type: "open-email-from-search",
            id,
            markRead: settings.markReadOnOpen,
            openInTab: settings.openInTab,
          }
        : {
            type: "open-email",
            id,
            markRead: settings.markReadOnOpen,
            openInTab: settings.openInTab,
          };
      dispatch(action);
      if (settings.markReadOnOpen) notifyOpen(id);
    },
    [dispatch, settings.markReadOnOpen, settings.openInTab],
  );

  const toggleSettings = useCallback(() => {
    dispatch(state.settingsPage == null ? { type: "open-settings" } : { type: "close-settings" });
  }, [dispatch, state.settingsPage]);

  const cycleSettingsPage = useCallback(
    (direction: number) => {
      const current = state.settingsPage ?? "general";
      const index = SETTINGS_PAGES.indexOf(current);
      const next =
        SETTINGS_PAGES[(index + direction + SETTINGS_PAGES.length) % SETTINGS_PAGES.length];
      dispatch({ type: "set-settings-page", page: next });
    },
    [dispatch, state.settingsPage],
  );

  const handleSignIn = useCallback(() => {
    const bridge = getNoriBridge();
    if (!bridge?.signin) return;
    setSigningIn(true);
    bridge
      .signin()
      .then((address) => setAccountAddress(address))
      .catch(() => undefined)
      .finally(() => setSigningIn(false));
  }, []);

  const handleSignOut = useCallback(() => {
    const bridge = getNoriBridge();
    if (!bridge?.signout) return;
    bridge
      .signout()
      .then(() => setAccountAddress(null))
      .catch(() => undefined);
  }, []);

  // Global shortcuts (mail_app.rs on_action handlers).
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const inField =
        target != null &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
      // Search dialog handles its own keys.
      if (state.overlay === "search") {
        if (e.key === "Escape") closeOverlay();
        return;
      }
      // Compose dialog: only Escape dismisses globally; fields own the rest.
      if (state.overlay === "compose") {
        if (e.key === "Escape") closeOverlay();
        return;
      }
      const mod_ = e.ctrlKey || e.metaKey;
      if (mod_ && e.shiftKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        toggleSettings();
        return;
      }
      if (mod_ && (e.key === "w" || e.key === "W")) {
        e.preventDefault();
        dispatch({ type: "close-active-tab" });
        return;
      }
      if (mod_ && e.key === "Tab") {
        e.preventDefault();
        dispatch({ type: "cycle-tab", direction: e.shiftKey ? -1 : 1 });
        return;
      }
      if (mod_ && (e.key === "b" || e.key === "B")) {
        e.preventDefault();
        setSidebarVisible((v) => !v);
        resizeRef.current = null;
        return;
      }
      if (inField) {
        if (e.key === "Escape") (target as HTMLElement).blur();
        return;
      }
      // Settings owns its keys while open: Up/Down move between pages,
      // Escape closes, and the mail keys stay quiet underneath.
      if (state.settingsPage != null) {
        if (e.key === "Escape") dispatch({ type: "close-settings" });
        else if (e.key === "ArrowDown" || e.key === "Down") {
          e.preventDefault();
          cycleSettingsPage(1);
        } else if (e.key === "ArrowUp" || e.key === "Up") {
          e.preventDefault();
          cycleSettingsPage(-1);
        }
        return;
      }
      if (e.key === "j") dispatch({ type: "move-selection", delta: 1 });
      else if (e.key === "k") dispatch({ type: "move-selection", delta: -1 });
      else if (e.key === "Enter") {
        const email = summaries[state.selectedIndex];
        if (email) openEmail(email.id);
      } else if (e.key === "c") openComposeDefault();
      else if (e.key === "/") {
        e.preventDefault();
        dispatch({ type: "open-search" });
      } else if (e.key === "Escape") {
        if (state.workspaceView.kind === "email") dispatch({ type: "go-back" });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    dispatch,
    summaries,
    state.selectedIndex,
    state.workspaceView,
    state.overlay,
    state.settingsPage,
    closeOverlay,
    openComposeDefault,
    openEmail,
    toggleSettings,
    cycleSettingsPage,
  ]);

  // Sidebar drag resize (mail_app.rs mouse move/up handlers).
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const r = resizeRef.current;
      if (!r) return;
      setSidebarWidth(clampSidebarWidth(r.startWidth + (e.clientX - r.startX)));
    };
    const onUp = () => {
      resizeRef.current = null;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  const tabs = state.tabs
    .map((id) => {
      const email = state.emails.find((e) => e.id === id);
      return email ? { id, subject: email.subject } : null;
    })
    .filter((t): t is { id: number; subject: string } => t !== null);

  const settingsOpen = state.settingsPage != null;
  const settingsPage: SettingsPage = state.settingsPage ?? "general";
  const account: SettingsAccount = signingIn
    ? { status: "fetching", address: accountAddress ?? undefined }
    : accountAddress != null
      ? {
          status: "connected",
          address: accountAddress,
          mailCount: state.emails.length,
          labelCount: 0,
        }
      : { status: "disconnected" };
  const visibleCounts = settings.unreadBadges
    ? counts
    : { inbox: 0, starred: 0, sent: 0, drafts: 0, archive: 0, trash: 0 };

  return (
    <div
      id="mail-app"
      className="nori-app"
      data-theme={settings.lightMode ? "light" : "dark"}
    >
      <div className="nori-app-main">
        <Sidebar
          selected={state.selectedMailbox}
          counts={visibleCounts}
          width={sidebarWidth}
          visible={sidebarVisible}
          mailboxesCollapsed={mailboxesCollapsed}
          onMailbox={(mailbox) => dispatch({ type: "select-mailbox", mailbox })}
          onSearch={() => dispatch({ type: "open-search" })}
          onCompose={openComposeDefault}
          onSettings={() => dispatch({ type: "open-settings" })}
          onToggle={() => {
            setSidebarVisible((v) => !v);
            resizeRef.current = null;
          }}
          onToggleGroup={() => setMailboxesCollapsed((v) => !v)}
          onBeginResize={(startX) => {
            resizeRef.current = { startX, startWidth: sidebarWidth };
          }}
          onResizeStep={(delta) => setSidebarWidth((w) => clampSidebarWidth(w + delta))}
        />

        <div className="nori-app-right">
          <TopBar
            title={
              settingsOpen
                ? settingsPageLabel(settingsPage)
                : state.workspaceView.kind === "email" && activeEmail
                  ? activeEmail.subject
                  : mailboxLabel(state.selectedMailbox)
            }
            prefix={settingsOpen ? "Settings" : undefined}
            sidebarVisible={sidebarVisible}
            onToggleSidebar={() => {
              setSidebarVisible((v) => !v);
              resizeRef.current = null;
            }}
          />
          {settingsOpen ? (
            <div className="nori-workspace">
              <SettingsView
                page={settingsPage}
                settings={settings}
                account={account}
                onPage={(page) => dispatch({ type: "set-settings-page", page })}
                onSet={(setting, enabled) =>
                  setSettings({ [setting]: enabled } as Partial<typeof settings>)
                }
                onSignIn={handleSignIn}
                onSignOut={handleSignOut}
              />
            </div>
          ) : (
            <div className="nori-workspace">
              <EmailTabs
                tabs={tabs}
                active={state.activeTab}
                onSelect={(id) => openEmail(id)}
                onClose={(id) => dispatch({ type: "close-tab", id })}
                onNew={openComposeDefault}
              />
              <div className="nori-workspace-body">
                {state.workspaceView.kind === "email" && activeEmail ? (
                  <EmailView
                    email={activeEmail}
                    showSender={settings.showSender}
                    onReply={() =>
                      dispatch({ type: "open-compose", seed: replySeed(activeEmail, false, false) })
                    }
                    onReplyAll={() =>
                      dispatch({ type: "open-compose", seed: replySeed(activeEmail, true, false) })
                    }
                    onForward={() =>
                      dispatch({ type: "open-compose", seed: replySeed(activeEmail, false, true) })
                    }
                  />
                ) : (
                  <EmailList
                    rows={summaries}
                    selectedIndex={state.selectedIndex}
                    compact={settings.compactRows}
                    onOpen={(id) => openEmail(id)}
                    onStar={(id) => {
                      dispatch({ type: "toggle-star", id });
                      notifyToggleStar(id);
                    }}
                    onSelectIndex={(index) => dispatch({ type: "set-selected-index", index })}
                  />
                )}
              </div>
              {!sidebarVisible && (
                <div className="nori-show-sidebar">
                  <Button
                    id="show-sidebar"
                    label=""
                    dense
                    buttonStyle="ghost"
                    ariaLabel="Show sidebar"
                    onClick={() => setSidebarVisible(true)}
                    icon={<Icon path="icons/panel-left.svg" size={14} color="var(--nori-muted)" />}
                  />
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {state.overlay != null && (
        <div className="nori-overlay" onMouseDown={closeOverlay}>
          {state.overlay === "search" ? (
            <div className="nori-overlay-top">
              <SearchDialog
                emails={state.emails}
                onOpen={(id) => openEmail(id, { fromSearch: true })}
                onDismiss={closeOverlay}
              />
            </div>
          ) : (
            <div className="nori-overlay-top">
              <ComposeDialog
                seed={state.composeSeed ?? { to: "", subject: "", body: "" }}
                onDismiss={closeOverlay}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
