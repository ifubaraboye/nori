import type { EmailId } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./EmailTabs.css";

interface EmailTabsProps {
  tabs: { id: EmailId; subject: string }[];
  active: EmailId | null;
  onSelect: (id: EmailId) => void;
  onClose: (id: EmailId) => void;
  onNew: () => void;
}

/** Port of components/email_tabs.rs (36px bar, 118-220px tabs, accent underline). */
export function EmailTabs({ tabs, active, onSelect, onClose, onNew }: EmailTabsProps) {
  if (tabs.length === 0) return null;
  return (
    <div id="email-tabs" className="nori-tabs" role="tablist" aria-label="Open emails">
      <div className="nori-tabs-scroll">
        {tabs.map(({ id, subject }) => {
          const isActive = active === id;
          return (
            <div
              key={id}
              id={`email-tab-${id}`}
              role="tab"
              aria-selected={isActive}
              tabIndex={0}
              onClick={() => onSelect(id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(id);
                }
              }}
              className={isActive ? "nori-tab nori-tab--active" : "nori-tab"}
            >
              <span className="nori-tab-label">{subject}</span>
              <Button
                id={`close-tab-${id}`}
                label=""
                dense
                buttonStyle="ghost"
                ariaLabel={`Close ${subject}`}
                onClick={() => onClose(id)}
                icon={<Icon path="icons/close.svg" size={11} color="var(--nori-faint)" />}
              />
            </div>
          );
        })}
        <Button
          id="new-email-tab"
          label=""
          dense
          buttonStyle="ghost"
          ariaLabel="Compose"
          onClick={onNew}
          icon={<Icon path="icons/plus.svg" size={14} color="var(--nori-muted)" />}
        />
      </div>
    </div>
  );
}
