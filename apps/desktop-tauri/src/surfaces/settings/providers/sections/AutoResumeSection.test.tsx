import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setProviderAutoResumeAfterQuotaReset } from "../../../../lib/tauri";
import { AutoResumeSection } from "./AutoResumeSection";

vi.mock("../../../../lib/tauri", () => ({
  setProviderAutoResumeAfterQuotaReset: vi.fn(),
}));

describe("AutoResumeSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(setProviderAutoResumeAfterQuotaReset).mockResolvedValue(undefined);
  });

  it("renders the opt-in control for Codex and persists a change", async () => {
    const onChanged = vi.fn();
    render(
      <AutoResumeSection
        providerId="codex"
        enabled={false}
        available={true}
        disabled={false}
        t={(key) => key}
        onChanged={onChanged}
      />,
    );

    const checkbox = screen.getByRole("checkbox");
    expect(checkbox).not.toBeChecked();
    await act(async () => {
      fireEvent.click(checkbox);
    });

    await vi.waitFor(() =>
      expect(setProviderAutoResumeAfterQuotaReset).toHaveBeenCalledWith("codex", true),
    );
    expect(onChanged).toHaveBeenCalledOnce();
  });

  it("does not render for providers without CLI session resume", () => {
    const { container } = render(
      <AutoResumeSection
        providerId="antigravity"
        enabled={false}
        available={false}
        disabled={false}
        t={(key) => key}
        onChanged={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("does not render when the active credential lane cannot be correlated", () => {
    const { container } = render(
      <AutoResumeSection
        providerId="codex"
        enabled={false}
        available={false}
        disabled={false}
        t={(key) => key}
        onChanged={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });
});
