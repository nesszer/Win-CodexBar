import type { PaceSnapshot } from "../../types/bridge";

/**
 * Broad pace categories used for the tray / pop-out pace badge.
 *
 * egui's `native_ui` surfaces a 7-stage pace enum (`on_track` through
 * `far_behind` / `far_ahead`); the pop-out card only has room for a short
 * pill, so we collapse those stages into four buckets with distinct colours.
 */
export type PaceCategory = "slow" | "steady" | "racing" | "burning";

export function paceCategory(stage: PaceSnapshot["stage"]): PaceCategory {
  switch (stage) {
    case "slightly_behind":
    case "behind":
    case "far_behind":
      return "slow";
    case "on_track":
      return "steady";
    case "slightly_ahead":
    case "ahead":
      return "racing";
    case "far_ahead":
      return "burning";
    default:
      return "steady";
  }
}
