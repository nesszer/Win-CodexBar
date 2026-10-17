import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LocaleKey } from "../../../../i18n/keys";
import { getProviderWorkspaceId } from "../../../../lib/tauri";
import { CredentialsDispatcher } from "./CredentialsDispatcher";

vi.mock("../../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../lib/tauri")>()),
  getProviderWorkspaceId: vi.fn(),
  setProviderWorkspaceId: vi.fn(),
}));

const t = (key: LocaleKey) => key as string;

describe("CredentialsDispatcher", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getProviderWorkspaceId).mockResolvedValue(null);
  });

  it.each([
    ["flora", "FLORA workspace"],
    ["fireworks", "Fireworks account"],
    ["cosmic", "CosmicProjectIdTitle"],
    ["openaiapi", "OpenAiAdminApiTitle"],
  ])("shows the workspace field for %s", async (providerId, title) => {
    render(<CredentialsDispatcher providerId={providerId} t={t} />);
    expect(await screen.findByText(title)).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("renders nothing for providers without a credentials extra", () => {
    const { container } = render(<CredentialsDispatcher providerId="claude" t={t} />);
    expect(container).toBeEmptyDOMElement();
  });
});
