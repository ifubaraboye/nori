import type { EmailId } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./EmailTabs.css";

interface EmailTabsProps {
  tabs: { id: EmailId; subject: string }[];
  /** The provisional tab, rendered last and in italics (email_tabs.rs). */
  preview: EmailId | null;
  active: EmailId | null;
  onSelect: (id: EmailId) => void;
  onClose: (id: EmailId) => void;
}

/** Port of components/email_tabs.rs (40px bar, 118-220px tabs, inset strip). */
export function EmailTabs({ tabs, preview, active, onSelect, onClose }: EmailTabsProps) {
  if (tabs.length === 0) return null;
  return (
    <div id="email-tabs" className="nori-tabs" role="tablist" aria-label="Open emails">
      <div className="nori-tabs-scroll">
        {tabs.map(({ id, subject }) => {
          const isActive = active === id;
          const isPreview = preview === id;
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
              className={[
                "nori-tab",
                isActive ? "nori-tab--active" : "",
                isPreview ? "nori-tab--preview" : "",
              ]
                .filter(Boolean)
                .join(" ")}
            >
              <span className="nori-tab-label">{subject}</span>
              <Button
                id={`close-tab-${id}`}
                label=""
                dense
                buttonStyle="ghost"
                ariaLabel={`Close ${subject}`}
                onClick={() => onClose(id)}
                icon={<Icon path="icons/close.svg" size={11.55} color="var(--nori-ghost)" />}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
