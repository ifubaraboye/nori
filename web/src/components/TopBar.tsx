import { Icon } from "./Icon";
import "./TopBar.css";

interface TopBarProps {
  title: string;
  prefix?: string;
  sidebarVisible: boolean;
  onToggleSidebar: () => void;
}

/** Port of components/top_bar.rs: title only (+ sidebar toggle while hidden). */
export function TopBar({ title, prefix, sidebarVisible, onToggleSidebar }: TopBarProps) {
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
      <div id="top-bar-title" className="nori-topbar-title">
        {prefix != null && (
          <>
            <span id="top-bar-prefix" className="nori-topbar-prefix">
              {prefix}
            </span>
            <span className="nori-topbar-separator"> / </span>
          </>
        )}
        {title}
      </div>
    </div>
  );
}
