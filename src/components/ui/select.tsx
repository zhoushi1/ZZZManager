import type { SelectHTMLAttributes } from "react";
import { cn } from "../../lib/utils";

/**
 * A native `<select>` styled to match the app's Input. Kept native (rather than
 * a headless popover) so it stays lightweight and accessible; callers provide
 * `<option>` children.
 */
export function Select({
  className,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cn(
        "h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}
