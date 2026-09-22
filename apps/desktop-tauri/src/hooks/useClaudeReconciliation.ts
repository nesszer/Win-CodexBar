import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { claudeReconciliationState } from "../lib/tauri";
import type { ClaudeReconciliationSnapshot } from "../types/bridge";

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

  useEffect(() => {
    let mounted = true;
    const unlisten = listen<ClaudeReconciliationSnapshot>(
      "claude-reconciliation-changed",
      event => {
        if (mounted) accept(event.payload);
      },
    );
    void claudeReconciliationState()
      .then(current => {
        if (mounted && current) accept(current);
      })
      .catch(() => {});
    return () => {
      mounted = false;
      void unlisten.then(dispose => dispose()).catch(() => {});
    };
  }, [accept]);

  return {
    snapshot,
    accept,
    reconciling: snapshot?.status === "pending",
  };
}
