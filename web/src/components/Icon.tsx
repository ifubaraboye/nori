import archiveSvg from "../assets/icons/archive.svg?raw";
import appearanceSvg from "../assets/icons/appearance.svg?raw";
import bellSvg from "../assets/icons/bell.svg?raw";
import chevronDownSvg from "../assets/icons/chevron-down.svg?raw";
import chevronLeftSvg from "../assets/icons/chevron-left.svg?raw";
import chevronRightSvg from "../assets/icons/chevron-right.svg?raw";
import closeSvg from "../assets/icons/close.svg?raw";
import composeSvg from "../assets/icons/compose.svg?raw";
import draftsSvg from "../assets/icons/drafts.svg?raw";
import ellipsisSvg from "../assets/icons/ellipsis.svg?raw";
import forwardSvg from "../assets/icons/forward.svg?raw";
import gearSvg from "../assets/icons/gear.svg?raw";
import inboxSvg from "../assets/icons/inbox.svg?raw";
import infoSvg from "../assets/icons/info.svg?raw";
import linkSvg from "../assets/icons/link.svg?raw";
import mailSvg from "../assets/icons/mail.svg?raw";
import panelLeftSvg from "../assets/icons/panel-left.svg?raw";
import paperclipSvg from "../assets/icons/paperclip.svg?raw";
import plusSvg from "../assets/icons/plus.svg?raw";
import replyAllSvg from "../assets/icons/reply-all.svg?raw";
import replySvg from "../assets/icons/reply.svg?raw";
import resetSvg from "../assets/icons/reset.svg?raw";
import searchSvg from "../assets/icons/search.svg?raw";
import sendSvg from "../assets/icons/send.svg?raw";
import sentSvg from "../assets/icons/sent.svg?raw";
import settingsSvg from "../assets/icons/settings.svg?raw";
import starFilledSvg from "../assets/icons/star-filled.svg?raw";
import starSvg from "../assets/icons/star.svg?raw";
import trashSvg from "../assets/icons/trash.svg?raw";
import userSvg from "../assets/icons/user.svg?raw";

import "./Icon.css";

const ICONS: Record<string, string> = {
  "icons/archive.svg": archiveSvg,
  "icons/appearance.svg": appearanceSvg,
  "icons/bell.svg": bellSvg,
  "icons/chevron-down.svg": chevronDownSvg,
  "icons/chevron-left.svg": chevronLeftSvg,
  "icons/chevron-right.svg": chevronRightSvg,
  "icons/close.svg": closeSvg,
  "icons/compose.svg": composeSvg,
  "icons/drafts.svg": draftsSvg,
  "icons/ellipsis.svg": ellipsisSvg,
  "icons/forward.svg": forwardSvg,
  "icons/gear.svg": gearSvg,
  "icons/inbox.svg": inboxSvg,
  "icons/info.svg": infoSvg,
  "icons/link.svg": linkSvg,
  "icons/mail.svg": mailSvg,
  "icons/panel-left.svg": panelLeftSvg,
  "icons/paperclip.svg": paperclipSvg,
  "icons/plus.svg": plusSvg,
  "icons/reply-all.svg": replyAllSvg,
  "icons/reply.svg": replySvg,
  "icons/reset.svg": resetSvg,
  "icons/search.svg": searchSvg,
  "icons/send.svg": sendSvg,
  "icons/sent.svg": sentSvg,
  "icons/settings.svg": settingsSvg,
  "icons/star-filled.svg": starFilledSvg,
  "icons/star.svg": starSvg,
  "icons/trash.svg": trashSvg,
  "icons/user.svg": userSvg,
};

interface IconProps {
  path: string;
  size: number;
  color: string;
}

/**
 * Port of components/icon.rs. The Rust SVGs use `stroke="currentColor"`,
 * so inlining the raw SVG and setting `color` preserves the tint.
 */
export function Icon({ path, size, color }: IconProps) {
  const svg = ICONS[path] ?? "";
  return (
    <span
      aria-hidden="true"
      className="nori-icon"
      style={{
        width: size,
        height: size,
        color,
      }}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
