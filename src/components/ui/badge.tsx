import type { HTMLAttributes } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex h-5 shrink-0 items-center gap-1 rounded-md border border-transparent px-1.5 text-xs font-medium whitespace-nowrap",
  {
    variants: {
      variant: {
        neutral: "border-border bg-secondary text-secondary-foreground",
        success: "bg-success-muted text-success",
        warning: "bg-warning-muted text-warning-foreground",
        danger: "bg-destructive/10 text-destructive",
        info: "bg-info-muted text-info",
      },
    },
    defaultVariants: {
      variant: "neutral",
    },
  },
);

export type BadgeProps = HTMLAttributes<HTMLSpanElement> &
  VariantProps<typeof badgeVariants>;

export function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}
