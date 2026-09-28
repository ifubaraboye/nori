import type { Email } from "../types/mail";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./EmailView.css";

interface EmailViewProps {
  email: Email;
  showSender: boolean;
  onReply: () => void;
  onReplyAll: () => void;
  onForward: () => void;
  onTogglePin: () => void;
  onArchive: () => void;
}

/** Port of views/email_view.rs. */
export function EmailView({
  email,
  showSender,
  onReply,
  onReplyAll,
  onForward,
  onTogglePin,
  onArchive,
}: EmailViewProps) {
  return (
    <div id={`email-view-${email.id}`} className="nori-email-view" tabIndex={0}>
      <div className="nori-email-view-inner">
        <h1 className="nori-email-view-subject">{email.subject}</h1>
        <div className="nori-email-view-meta">
          <div className="nori-email-view-from">
            {showSender && (
              <div className="nori-email-view-sender">
                {email.sender} &lt;{email.address}&gt;
              </div>
            )}
            <div className="nori-email-view-to">To: {email.recipients.join(", ")}</div>
          </div>
          <div style={{ flex: 1 }} />
          <div className="nori-email-view-date">{email.fullDate}</div>
        </div>
        <div className="nori-email-view-rule" />
        <div className="nori-email-view-body">
          {email.body.map((paragraph, i) => (
            <p key={i}>{paragraph}</p>
          ))}
        </div>
        <div className="nori-email-view-rule" />
        <div className="nori-email-view-actions">
          <Button
            id="reply-button"
            label="Reply"
            dense
            buttonStyle="subtle"
            onClick={onReply}
            icon={<Icon path="icons/reply.svg" size={13} color="var(--nori-muted)" />}
          />
          <Button
            id="reply-all-button"
            label="Reply All"
            dense
            buttonStyle="subtle"
            onClick={onReplyAll}
            icon={<Icon path="icons/reply-all.svg" size={13} color="var(--nori-muted)" />}
          />
          <Button
            id="forward-button"
            label="Forward"
            dense
            buttonStyle="subtle"
            onClick={onForward}
            icon={<Icon path="icons/forward.svg" size={13} color="var(--nori-muted)" />}
          />
          <span style={{ flex: 1 }} />
          <Button
            id="email-pin-button"
            label={email.pinned ? "Unpin" : "Pin"}
            dense
            buttonStyle="subtle"
            onClick={onTogglePin}
            icon={
              <Icon
                path={email.pinned ? "icons/pin.svg" : "icons/pin-off.svg"}
                size={13}
                color={email.pinned ? "var(--nori-accent)" : "var(--nori-muted)"}
              />
            }
          />
          <Button
            id="archive-button"
            label="Archive"
            dense
            buttonStyle="subtle"
            onClick={onArchive}
            icon={<Icon path="icons/archive.svg" size={13} color="var(--nori-muted)" />}
          />
        </div>
      </div>
    </div>
  );
}
