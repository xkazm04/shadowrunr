import type { ButtonHTMLAttributes, Ref } from "react";
import { cx } from "./cx";
import styles from "./Button.module.css";

/** Paper with an ink offset (`secondary`), the page accent (`primary`), or ink with an accent offset. */
export type ButtonVariant = "primary" | "secondary" | "ink";
/** `compact` sits inline with running text and takes its size from it. */
export type ButtonSize = "regular" | "compact";

/** The native button's classes, for an element that is not a `<button>` (a link that acts as one). */
export function buttonClass(variant: ButtonVariant = "secondary", size: ButtonSize = "regular", className?: string): string {
  return cx(styles.button, variant !== "secondary" && styles[variant], size === "compact" && styles.compact, className);
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  readonly variant?: ButtonVariant;
  readonly size?: ButtonSize;
  readonly ref?: Ref<HTMLButtonElement>;
}

/** The console's native button. `type` defaults to `button`; a form's submit says so. */
export function Button({ variant = "secondary", size = "regular", className, type = "button", ...rest }: ButtonProps) {
  return <button type={type} className={buttonClass(variant, size, className)} data-variant={variant} {...rest} />;
}
