import { useEffect, useRef } from "react";
import type { EmailId, EmailSummary } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./EmailList.css";

interface EmailListProps {
  rows: EmailSummary[];
  selectedIndex: number;
  onOpen: (id: EmailId) => void;
  onStar: (id: EmailId) => void;
  onSelectIndex: (index: number) => void;
}

/** Port of views/inbox.rs + components/email_row.rs. */
export function EmailList({ rows, selectedIndex, onOpen, onStar, onSelectIndex }: EmailListProps) {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(
      `[data-email-index="${selectedIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  return (
    <div id="inbox" className="nori-inbox" role="list" aria-label="Email list" tabIndex={0}>
      <div id="email-list" ref={listRef} className="nori-email-list">
        {rows.map((row, index) => {
          const selected = index === selectedIndex;
          const label = `${row.sender}: ${row.subject}`;
          return (
            <div
              key={row.id}
              id={`email-row-${row.id}`}
              data-email-index={index}
              role="listitem"
              aria-label={label}
              aria-selected={selected}
              tabIndex={-1}
              onClick={() => {
                onSelectIndex(index);
                onOpen(row.id);
              }}
              className={selected ? "nori-email-row nori-email-row--selected" : "nori-email-row"}
            >
              <div className="nori-email-row-main">
                <div className="nori-email-row-top">
                  <span
                    className={
                      row.unread
                        ? "nori-email-row-sender nori-email-row-sender--unread"
                        : "nori-email-row-sender"
                    }
                  >
                    {row.sender}
                  </span>
                  <span className="nori-email-row-time">{row.timestamp}</span>
                </div>
                <div
                  className={
                    row.unread
                      ? "nori-email-row-subject nori-email-row-subject--unread"
                      : "nori-email-row-subject"
                  }
                >
                  {row.subject}
                </div>
                <div className="nori-email-row-preview">{row.preview}</div>
              </div>
              <Button
                id={`star-${row.id}`}
                label=""
                dense
                buttonStyle="ghost"
                ariaLabel={row.starred ? "Remove star" : "Add star"}
                onClick={() => onStar(row.id)}
                icon={
                  <Icon
                    path={row.starred ? "icons/star-filled.svg" : "icons/star.svg"}
                    size={14}
                    color={row.starred ? "var(--nori-accent)" : "var(--nori-faint)"}
                  />
                }
              />
            </div>
          );
        })}
        {rows.length === 0 && (
          <div className="nori-email-empty">No messages in this mailbox.</div>
        )}
      </div>
    </div>
  );
}
