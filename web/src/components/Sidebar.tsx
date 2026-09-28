import { useCallback } from "react";
import { MAILBOX_NAV_ITEMS, mailboxLabel, type Mailbox } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./Sidebar.css";

export const SIDEBAR_DEFAULT_WIDTH = 252;
export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 420;

export function clampSidebarWidth(width: number): number {
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width));
}

const MAILBOX_ICONS: Record<Mailbox, string> = {
  inbox: "icons/inbox.svg",
  starred: "icons/star.svg",
  sent: "icons/sent.svg",
  drafts: "icons/drafts.svg",
  archive: "icons/archive.svg",
  trash: "icons/trash.svg",
};

interface SidebarProps {
  selected: Mailbox;
  counts: Record<Mailbox, number>;
  width: number;
  visible: boolean;
  mailboxesCollapsed: boolean;
  onMailbox: (mailbox: Mailbox) => void;
  onSearch: () => void;
  onCompose: () => void;
  onSettings: () => void;
  onToggle: () => void;
  onToggleGroup: () => void;
  onBeginResize: (startX: number) => void;
  onResizeStep: (delta: number) => void;
}

/** Port of components/sidebar.rs. */
export function Sidebar({
  selected,
  counts,
  width,
  visible,
  mailboxesCollapsed,
  onMailbox,
  onSearch,
  onCompose,
  onToggle,
  onToggleGroup,
  onBeginResize,
  onResizeStep,
  onSettings,
}: SidebarProps) {
  const clamped = clampSidebarWidth(width);

  const onResizeKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const step = e.shiftKey ? 20 : 8;
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        onResizeStep(-step);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        onResizeStep(step);
      } else if (e.key === "Home") {
        e.preventDefault();
        onResizeStep(SIDEBAR_MIN_WIDTH - clamped);
      } else if (e.key === "End") {
        e.preventDefault();
        onResizeStep(SIDEBAR_MAX_WIDTH - clamped);
      }
    },
    [clamped, onResizeStep],
  );

  return (
    <div
      id="mail-sidebar-shell"
      className={visible ? "nori-sidebar-shell" : "nori-sidebar-shell nori-sidebar-shell--hidden"}
      style={{ width: visible ? clamped : 0 }}
    >
      <div className="nori-sidebar-content" style={{ width: clamped }} aria-hidden={!visible}>
        <div className="nori-sidebar-header">
          <button
            id="sidebar-hide"
            type="button"
            className="nori-sidebar-hide"
            aria-label="Hide sidebar"
            onClick={onToggle}
          >
            <Icon path="icons/panel-left.svg" size={18} color="var(--nori-muted)" />
          </button>
        </div>

        <div className="nori-sidebar-compose">
          <button
            id="sidebar-compose"
            type="button"
            className="nori-sidebar-row"
            aria-label="Compose"
            onClick={onCompose}
          >
            <span className="nori-sidebar-row-icon">
              <Icon path="icons/compose.svg" size={16} color="var(--nori-muted)" />
            </span>
            <span className="nori-sidebar-row-label">Compose</span>
          </button>
        </div>

        <nav id="mail-sidebar-nav" className="nori-sidebar-nav" aria-label="Mailboxes">
          <div className="nori-sidebar-search">
            <button
              id="sidebar-search"
              type="button"
              className="nori-sidebar-row"
              aria-label="Search"
              onClick={onSearch}
            >
              <span className="nori-sidebar-row-icon">
                <Icon path="icons/search.svg" size={16} color="var(--nori-muted)" />
              </span>
              <span className="nori-sidebar-row-label">Search</span>
            </button>
          </div>
          <div style={{ height: 10 }} />
          <div className="nori-sidebar-group">
            <button
              id="sidebar-mailboxes-toggle"
              type="button"
              className="nori-sidebar-group-toggle"
              aria-label="Mailboxes group"
              onClick={onToggleGroup}
              onKeyDown={(e) => {
                if ((e.key === "ArrowLeft" && !mailboxesCollapsed) || (e.key === "ArrowRight" && mailboxesCollapsed)) {
                  onToggleGroup();
                }
              }}
            >
              <span>Mailboxes</span>
              <Icon
                path={mailboxesCollapsed ? "icons/chevron-right.svg" : "icons/chevron-down.svg"}
                size={12}
                color="var(--nori-faint)"
              />
            </button>
          </div>
          {!mailboxesCollapsed &&
            MAILBOX_NAV_ITEMS.map((mailbox, index) => {
              const count = counts[mailbox];
              const isSelected = selected === mailbox;
              return (
                <div key={mailbox} className="nori-sidebar-row-wrap">
                  <button
                    id={`sidebar-mailbox-${index}`}
                    type="button"
                    role="button"
                    aria-label={`${mailboxLabel(mailbox)} (${count} messages)`}
                    aria-selected={isSelected}
                    onClick={() => onMailbox(mailbox)}
                    className={
                      isSelected
                        ? "nori-sidebar-row nori-sidebar-row--selected"
                        : "nori-sidebar-row"
                    }
                  >
                    <span className="nori-sidebar-row-icon">
                      <Icon
                        path={MAILBOX_ICONS[mailbox]}
                        size={16}
                        color={isSelected ? "var(--nori-text)" : "var(--nori-muted)"}
                      />
                    </span>
                    <span className="nori-sidebar-row-label">{mailboxLabel(mailbox)}</span>
                    {count > 0 && <span className="nori-sidebar-row-count">{count}</span>}
                  </button>
                </div>
              );
            })}
        </nav>

        <div className="nori-sidebar-footer">
          <Button
            id="sidebar-settings"
            label=""
            buttonStyle="ghost"
            dense
            ariaLabel="Settings"
            onClick={onSettings}
            icon={<Icon path="icons/gear.svg" size={14} color="var(--nori-faint)" />}
          />
        </div>
      </div>

      {visible && (
        <div
          id="mail-sidebar-resize"
          role="separator"
          aria-label="Resize sidebar"
          aria-orientation="vertical"
          tabIndex={0}
          className="nori-sidebar-resize"
          onMouseDown={(e) => {
            e.stopPropagation();
            onBeginResize(e.clientX);
          }}
          onKeyDown={onResizeKeyDown}
        >
          <div className="nori-sidebar-resize-line" />
        </div>
      )}
    </div>
  );
}
