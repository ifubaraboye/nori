import { Icon } from "./Icon";
import "./TopBar.css";

interface TopBarProps {
  title: string;
  prefix?: string;
  sidebarVisible: boolean;
  refreshing: boolean;
  onToggleSidebar: () => void;
  onRefresh: () => void;
}

/**
 * Port of components/top_bar.rs: 42px bar, left-aligned
 * `Mailbox / Subject` breadcrumb, sidebar toggle while hidden,
 * refresh at the right end (dimmed mid-fetch).
 */
export function TopBar({
  title,
  prefix,
  sidebarVisible,
  refreshing,
  onToggleSidebar,
  onRefresh,
}: TopBarProps) {
  return (
    <div id="top-bar" className="nori-topbar" role="toolbar" aria-label="Top bar">
      {!sidebarVisible && (
        <button
          id="top-bar-sidebar"
          type="button"
          className="nori-topbar-toggle"
          aria-label="Toggle sidebar"
          onClick={onToggleSidebar}
        >
          <Icon path="icons/panel-left.svg" size={14} color="var(--nori-muted)" />
        </button>
      )}
      {prefix != null && (
        <>
          <div id="top-bar-prefix" className="nori-topbar-prefix">
            {prefix}
          </div>
          <div id="top-bar-separator" className="nori-topbar-separator">
            /
          </div>
        </>
      )}
      <div
        id="top-bar-title"
        className={
          prefix != null ? "nori-topbar-title" : "nori-topbar-title nori-topbar-title--bare"
        }
      >
        {title}
      </div>
      <div
        id="top-bar-refresh"
        role="button"
        aria-label="Fetch new mail"
        tabIndex={0}
        className={refreshing ? "nori-topbar-refresh nori-topbar-refresh--busy" : "nori-topbar-refresh"}
        onClick={onRefresh}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onRefresh();
          }
        }}
      >
        <Icon
          path="icons/reset.svg"
          size={14}
          color={refreshing ? "var(--nori-ghost)" : "var(--nori-muted)"}
        />
      </div>
    </div>
  );
}
