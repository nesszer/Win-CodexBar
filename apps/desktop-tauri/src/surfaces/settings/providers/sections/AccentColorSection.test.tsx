import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AccentColorSection } from "./AccentColorSection";

describe("AccentColorSection", () => {
  it("uses the native color input as the only color preview", () => {
    const { container } = render(
      <AccentColorSection
        providerId="codex"
        accentColor="#123456"
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(container.querySelector('input[type="color"]')).toHaveValue("#123456");
    expect(container.querySelector(".accent-color-swatch-row")).toBeNull();
    expect(container.querySelector(".accent-color-swatch")).toBeNull();
  });
});
