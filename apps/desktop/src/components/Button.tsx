import type { ButtonHTMLAttributes, ReactNode } from "react";

/**
 * - `ink`: the one main action on a screen (ADR 0051 §4).
 * - `default`: an outlined action.
 * - `quiet`: no border until hovered; row and toolbar actions.
 */
export type ButtonVariant = "ink" | "default" | "quiet";

type Base = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  variant?: ButtonVariant;
  size?: "md" | "sm";
  icon?: ReactNode;
};
/** A button with only an icon must say what it does. */
export type ButtonProps = Base & ({ children: ReactNode } | { children?: undefined; "aria-label": string });

const variants: Record<ButtonVariant, string> = {
  ink: "bg-ink text-on-ink border-transparent hover:opacity-90 forced-colors:border-[ButtonText]",
  default: "border-line-strong hover:bg-hover active:bg-press",
  quiet: "border-transparent text-text-2 hover:text-text hover:bg-hover active:bg-press",
};

export function Button({ variant = "default", size = "md", icon, children, className = "", type = "button", ...rest }: ButtonProps) {
  const iconOnly = children === undefined;
  const height = size === "md" ? "h-[30px]" : "h-6 text-xs rounded-md";
  const width = iconOnly ? (size === "md" ? "w-[30px] justify-center" : "w-6 justify-center") : size === "md" ? "px-3" : "px-2.5";
  return (
    <button
      type={type}
      className={`inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-control border text-[13px] font-medium transition-transform duration-75 active:scale-[.98] disabled:pointer-events-none disabled:opacity-40 ${height} ${width} ${variants[variant]} ${className}`}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
}
