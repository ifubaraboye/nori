import type { CSSProperties, ReactNode } from "react";
import "./Button.css";

export type ButtonStyle = "ghost" | "subtle" | "accent";

interface ButtonProps {
  id?: string;
  label?: string;
  icon?: ReactNode;
  buttonStyle?: ButtonStyle;
  disabled?: boolean;
  dense?: boolean;
  fullWidth?: boolean;
  ariaLabel?: string;
  title?: string;
  onClick?: (e: React.MouseEvent) => void;
  style?: CSSProperties;
}

/** Port of components/button.rs (Ghost/Subtle/Accent, dense 22px vs 28px). */
export function Button({
  id,
  label = "",
  icon,
  buttonStyle = "ghost",
  disabled = false,
  dense = false,
  fullWidth = false,
  ariaLabel,
  title,
  onClick,
  style,
}: ButtonProps) {
  return (
    <button
      id={id}
      type="button"
      role="button"
      aria-label={ariaLabel ?? (label || undefined)}
      title={title}
      disabled={disabled}
      onClick={(e) => {
        if (disabled) return;
        e.stopPropagation();
        onClick?.(e);
      }}
      style={style}
      className={[
        "nori-button",
        `nori-button--${buttonStyle}`,
        dense ? "nori-button--dense" : "nori-button--regular",
        fullWidth ? "nori-button--full" : "",
        disabled ? "nori-button--disabled" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {icon}
      {label !== "" && <span className="nori-button__label">{label}</span>}
    </button>
  );
}
