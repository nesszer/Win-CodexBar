import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LocaleKey } from "../../../../i18n/keys";
import type { ModelRateLimitSnapshot } from "../../../../types/bridge";
import { OpenAiModelLimitsEditor } from "./OpenAiModelLimitsEditor";

const t = (key: LocaleKey) => key as string;

function renderEditor(limits: ModelRateLimitSnapshot[] = [], onSave = vi.fn()) {
  const view = render(
    <OpenAiModelLimitsEditor limits={limits} disabled={false} t={t} onSave={onSave} />,
  );
  return { ...view, onSave };
}

describe("OpenAiModelLimitsEditor", () => {
  it("saves trimmed rows with a blank requests limit as null", () => {
    const { onSave } = renderEditor();
    fireEvent.click(screen.getByText("OpenAiLimitAdd"));
    const [model, tpm] = screen.getAllByRole("textbox");
    fireEvent.change(model, { target: { value: "  gpt-5 " } });
    fireEvent.change(tpm, { target: { value: "90000" } });
    fireEvent.click(screen.getByText("OpenAiLimitSave"));
    expect(onSave).toHaveBeenCalledWith([
      { model: "gpt-5", tokensPerMinute: 90_000, requestsPerMinute: null },
    ]);
  });

  it("shows the error and does not save a row with a bad tokens-per-minute value", () => {
    const { onSave, container } = renderEditor();
    fireEvent.click(screen.getByText("OpenAiLimitAdd"));
    const inputs = container.querySelectorAll("input");
    fireEvent.change(inputs[0], { target: { value: "gpt-5" } });
    fireEvent.change(inputs[1], { target: { value: "lots" } });
    fireEvent.click(screen.getByText("OpenAiLimitSave"));
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toBe("OpenAiLimitErrorTpm");
  });

  it("rejects a duplicate model and keeps the rows the user typed", () => {
    const { onSave, container } = renderEditor([
      { model: "gpt-5", tokensPerMinute: 1_000, requestsPerMinute: null },
    ]);
    fireEvent.click(screen.getByText("OpenAiLimitAdd"));
    const inputs = container.querySelectorAll("input");
    // Row 0 uses inputs 0-2; the new row 1 uses inputs 3-5.
    fireEvent.change(inputs[3], { target: { value: "gpt-5" } });
    fireEvent.change(inputs[4], { target: { value: "500" } });
    fireEvent.click(screen.getByText("OpenAiLimitSave"));
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toBe("OpenAiLimitErrorDuplicate");
    expect((container.querySelectorAll("input")[3] as HTMLInputElement).value).toBe("gpt-5");
  });

  it("removes a row before save", () => {
    const { onSave, container } = renderEditor([
      { model: "gpt-5", tokensPerMinute: 1_000, requestsPerMinute: 10 },
      { model: "gpt-4o", tokensPerMinute: 2_000, requestsPerMinute: null },
    ]);
    fireEvent.click(screen.getAllByText("OpenAiLimitRemove")[0]);
    expect(container.querySelectorAll(".openai-limits__row")).toHaveLength(1);
    fireEvent.click(screen.getByText("OpenAiLimitSave"));
    expect(onSave).toHaveBeenCalledWith([
      { model: "gpt-4o", tokensPerMinute: 2_000, requestsPerMinute: null },
    ]);
  });
});
