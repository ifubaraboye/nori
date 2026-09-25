import { useState } from "react";
import type { DraftSeed } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./ComposeDialog.css";

interface ComposeDialogProps {
  seed: DraftSeed;
  onDismiss: () => void;
}

/** Port of views/compose_view.rs (620x492 dialog, disabled Attach/Send). */
export function ComposeDialog({ seed, onDismiss }: ComposeDialogProps) {
  const [to, setTo] = useState(seed.to);
  const [subject, setSubject] = useState(seed.subject);
  const [body, setBody] = useState(seed.body);

  return (
    <div
      id="compose-dialog"
      role="dialog"
      aria-label="Compose"
      className="nori-compose"
      onMouseDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onDismiss();
        }
      }}
    >
      <div className="nori-compose-header">
        <span className="nori-compose-title">Compose</span>
        <span style={{ flex: 1 }} />
        <Button
          id="compose-close"
          label=""
          dense
          buttonStyle="ghost"
          ariaLabel="Close compose"
          onClick={onDismiss}
          icon={<Icon path="icons/close.svg" size={14} color="var(--nori-muted)" />}
        />
      </div>

      <div className="nori-compose-fields">
        <label className="nori-compose-field">
          <span className="nori-compose-label">To</span>
          <input
            id="compose-to"
            className="nori-compose-input"
            value={to}
            placeholder="To"
            autoFocus
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        <label className="nori-compose-field">
          <span className="nori-compose-label">Subject</span>
          <input
            id="compose-subject"
            className="nori-compose-input"
            value={subject}
            placeholder="Subject"
            onChange={(e) => setSubject(e.target.value)}
          />
        </label>
        <div className="nori-compose-message-wrap">
          <span className="nori-compose-label">Message</span>
          <textarea
            id="compose-message"
            className="nori-compose-message"
            value={body}
            placeholder="Message"
            onChange={(e) => setBody(e.target.value)}
          />
        </div>
      </div>

      <div className="nori-compose-footer">
        <Button
          id="compose-attach"
          label="Attach"
          dense
          buttonStyle="ghost"
          disabled
          icon={<Icon path="icons/paperclip.svg" size={13} color="var(--nori-faint)" />}
        />
        <span style={{ flex: 1 }} />
        <Button
          id="compose-send"
          label="Send ↗"
          dense
          buttonStyle="accent"
          disabled
          icon={<Icon path="icons/send.svg" size={13} color="var(--nori-faint)" />}
        />
      </div>
    </div>
  );
}
