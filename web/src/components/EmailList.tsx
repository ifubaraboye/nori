import { useEffect, useRef } from "react";
import type { EmailId, EmailSummary } from "../types/mail";
import { Icon } from "./Icon";
import "./EmailList.css";

interface EmailListProps {
  rows: EmailSummary[];
  selectedIndex: number;
  compact: boolean;
  onOpen: (id: EmailId) => void;
  onStar: (id: EmailId) => void;
  onSelectIndex: (index: number) => void;
}

/** Port of views/inbox.rs + components/email_row.rs. */
export function EmailList({ rows, selectedIndex, compact, onOpen, onStar, onSelectIndex }: EmailListProps) {
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
              className={[
                "nori-email-row",
                compact ? "nori-email-row--compact" : "nori-email-row--comfortable",
                selected ? "nori-email-row--selected" : "",
                row.unread ? "nori-email-row--unread" : "",
              ]
                .filter(Boolean)
                .join(" ")}
            >
              <div
                className={
                  row.unread ? "nori-email-row-accent nori-email-row-accent--unread" : "nori-email-row-accent"
                }
              />
              {compact ? (
                <div className="nori-email-row-compact">
                  <div id={`email-row-sender-${row.id}`} className="nori-email-row-compact-sender">
                    {row.sender}
                  </div>
                  <div className="nori-email-row-compact-subject-wrap">
                    <div
                      className={
                        row.unread
                          ? "nori-email-row-compact-subject nori-email-row-compact-subject--unread"
                          : "nori-email-row-compact-subject"
                      }
                    >
                      {row.subject}
                    </div>
                    {row.threadCount > 1 && (
                      <div className="nori-email-row-thread-count">({row.threadCount})</div>
                    )}
                    <div className="nori-email-row-dash">—</div>
                    <div className="nori-email-row-compact-preview">{row.preview}</div>
                  </div>
                </div>
              ) : (
                <div className="nori-email-row-comfortable">
                  <div
                    className={
                      row.unread
                        ? "nori-email-row-comfortable-sender nori-email-row-comfortable-sender--unread"
                        : "nori-email-row-comfortable-sender"
                    }
                  >
                    {row.sender}
                  </div>
                  <div className="nori-email-row-comfortable-subject-row">
                    <div
                      className={
                        row.unread
                          ? "nori-email-row-comfortable-subject nori-email-row-comfortable-subject--unread"
                          : "nori-email-row-comfortable-subject"
                      }
                    >
                      {row.subject}
                    </div>
                    {row.threadCount > 1 && (
                      <div className="nori-email-row-thread-count">({row.threadCount})</div>
                    )}
                  </div>
                  <div className="nori-email-row-comfortable-preview">{row.preview}</div>
                </div>
              )}
              {!compact && (
                <div className="nori-email-row-timestamp-wrap">
                  <div className="nori-email-row-timestamp">{row.timestamp}</div>
                </div>
              )}
              <div className="nori-email-row-trailing">
                <div id={`email-row-menu-${row.id}`}>
                  <button
                    id={`row-menu-${row.id}`}
                    type="button"
                    className="nori-email-row-icon-button"
                    aria-label={`More actions for ${row.sender}`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Icon path="icons/ellipsis.svg" size={14} color="var(--nori-ghost)" />
                  </button>
                </div>
                <button
                  id={`star-${row.id}`}
                  type="button"
                  className="nori-email-row-icon-button"
                  aria-label={row.starred ? "Remove star" : "Add star"}
                  onClick={(e) => {
                    e.stopPropagation();
                    onStar(row.id);
                  }}
                >
                  <Icon
                    path={row.starred ? "icons/star-filled.svg" : "icons/star.svg"}
                    size={16}
                    color={row.starred ? "var(--nori-accent)" : "var(--nori-ghost)"}
                  />
                </button>
              </div>
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
