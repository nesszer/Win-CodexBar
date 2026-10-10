import { vi } from "vitest";

/** vi.mock factory for @tauri-apps/api/event: listen resolves to a no-op unlisten. */
export const listen = vi.fn().mockResolvedValue(() => {});
