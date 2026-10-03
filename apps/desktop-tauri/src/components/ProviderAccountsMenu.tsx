import { useState, type ComponentPropsWithoutRef, type ReactNode } from "react";

type Props = Omit<ComponentPropsWithoutRef<"details">, "children" | "onToggle" | "open" | "title"> & {
  title: string;
  count: number;
  children: ReactNode;
  actions: ReactNode;
  onLayoutChange?: () => void;
};

/** Shared account list and footer for providers with managed logins. */
export default function ProviderAccountsMenu({ title, count, children, actions, onLayoutChange, ...props }: Props) {
  const [expanded, setExpanded] = useState(true);
  return (
    <details
      {...props}
      className="codex-menu-accounts"
      aria-label={title}
      open={expanded}
      onToggle={event => {
        setExpanded(event.currentTarget.open);
        onLayoutChange?.();
      }}
    >
      <summary className="codex-menu-accounts__summary">
        <span className="codex-menu-accounts__title">{title}</span>
        <span className="codex-menu-accounts__count">{count}</span>
      </summary>
      {children}
      <div className="codex-menu-accounts__actions">{actions}</div>
    </details>
  );
}
