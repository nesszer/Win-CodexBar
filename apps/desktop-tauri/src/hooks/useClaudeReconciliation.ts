import { useCallback, useEffect, useState } from "react";
import { claudeReconciliationState } from "../lib/tauri";
import type { ClaudeReconciliationSnapshot } from "../types/bridge";
import { useTauriEvent } from "./useTauriEvent";

const isTerminal = (snapshot: ClaudeReconciliationSnapshot) => snapshot.status !== "pending";

export function localClaudeReconciliationOutcome(
  snapshot: ClaudeReconciliationSnapshot | null,
  operationGeneration: number | null,
): ClaudeReconciliationSnapshot | null {
  if (
    !snapshot
    || snapshot.status === "pending"
    || snapshot.generation !== operationGeneration
  ) {
    return null;
  }
  return snapshot;
}

export function selectClaudeReconciliation(
  current: ClaudeReconciliationSnapshot | null,
  candidate: ClaudeReconciliationSnapshot,
): ClaudeReconciliationSnapshot {
  if (!current || candidate.generation > current.generation) return candidate;
  if (candidate.generation < current.generation) return current;
  if (isTerminal(current) || candidate.status === "pending") return current;
  return candidate;
}

export function useClaudeReconciliation() {
  const [snapshot, setSnapshot] = useState<ClaudeReconciliationSnapshot | null>(null);
  const accept = useCallback((candidate: ClaudeReconciliationSnapshot) => {
    setSnapshot(current => selectClaudeReconciliation(current, candidate));
  }, []);

  useTauriEvent<ClaudeReconciliationSnapshot>(
    "claude-reconciliation-changed",
    event => accept(event.payload),
    [accept],
  );

  useEffect(() => {
    let mounted = true;
    void claudeReconciliationState()
      .then(current => {
        if (mounted && current) accept(current);
      })
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, [accept]);

  return {
    snapshot,
    accept,
    reconciling: snapshot?.status === "pending",
  };
}
