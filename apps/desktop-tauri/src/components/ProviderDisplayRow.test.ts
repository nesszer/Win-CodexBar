import { describe, expect, it } from "vitest";
import type { ProviderDisplayDetailSnapshot } from "../types/bridge";
import { groupProviderDisplayDetails } from "./ProviderDisplayRow";

function row(id: string, sectionTitle: string | null = null): ProviderDisplayDetailSnapshot {
  return { id, sectionTitle, title: id, value: "1", secondaryValue: null, progress: null };
}

describe("groupProviderDisplayDetails", () => {
  it("starts a titled group only where the section changes", () => {
    const rows = [
      row("plain"),
      row("a", "Model activity"),
      row("b", "Model activity"),
      row("c", "Workspace spend"),
      row("d", null),
    ];

    const groups = groupProviderDisplayDetails(rows);

    expect(groups.map((group) => group.title)).toEqual([
      null,
      "Model activity",
      "Workspace spend",
      null,
    ]);
    expect(groups.map((group) => group.rows.map((detail) => detail.id))).toEqual([
      ["plain"],
      ["a", "b"],
      ["c"],
      ["d"],
    ]);
  });
});
