import { useCallback, useState } from "react";

export function useDraftNumber(value: number) {
  const [draft, setDraft] = useState(value);
  const [prev, setPrev] = useState(value);
  if (value !== prev) {
    setPrev(value);
    setDraft(value);
  }

  const commit = useCallback(
    (next: number, onCommit: (value: number) => void) => {
      if (next === value) return;
      onCommit(next);
    },
    [value],
  );

  return { draft, setDraft, commit };
}
