import { useState, useRef, useLayoutEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/utils";

interface TooltipProps {
  content: string;
  children: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
}

export function Tooltip({ content, children, side = "top" }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const triggerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!visible || !triggerRef.current || !tooltipRef.current) return;

    const trigger = triggerRef.current.getBoundingClientRect();
    const tooltip = tooltipRef.current.getBoundingClientRect();
    const gap = 8;
    const edgePadding = 8;

    let top = 0;
    let left = 0;

    switch (side) {
      case "top":
        top = trigger.top - tooltip.height - gap;
        left = trigger.left + trigger.width / 2 - tooltip.width / 2;
        break;
      case "bottom":
        top = trigger.bottom + gap;
        left = trigger.left + trigger.width / 2 - tooltip.width / 2;
        break;
      case "left":
        top = trigger.top + trigger.height / 2 - tooltip.height / 2;
        left = trigger.left - tooltip.width - gap;
        break;
      case "right":
        top = trigger.top + trigger.height / 2 - tooltip.height / 2;
        left = trigger.right + gap;
        break;
    }

    // Clamp to viewport with edge padding
    const maxTop = window.innerHeight - tooltip.height - edgePadding;
    const maxLeft = window.innerWidth - tooltip.width - edgePadding;

    top = Math.max(edgePadding, Math.min(top, maxTop));
    left = Math.max(edgePadding, Math.min(left, maxLeft));

    setPosition({ top, left });
  }, [visible, side]);

  return (
    <>
      <div
        ref={triggerRef}
        className="inline-flex"
        onMouseEnter={() => setVisible(true)}
        onMouseLeave={() => setVisible(false)}
      >
        {children}
      </div>
      {visible &&
        typeof document !== "undefined" &&
        createPortal(
          <div
            ref={tooltipRef}
            className={cn(
              "pointer-events-none fixed z-50 whitespace-nowrap rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md",
            )}
            style={{
              top: `${position.top}px`,
              left: `${position.left}px`,
            }}
          >
            {content}
          </div>,
          document.body,
        )}
    </>
  );
}
