import { useEffect, type DependencyList } from "react";
import { listen, type Event } from "@tauri-apps/api/event";

/**
 * Subscribes to a Tauri event for the lifetime of `deps`. Events that arrive
 * after cleanup (while `listen` is still resolving) are dropped, and an
 * unmount before registration finishes still unlistens.
 */
export function useTauriEvent<T = unknown>(
  event: string,
  handler: (event: Event<T>) => void,
  deps: DependencyList,
): void {
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    // `Promise.resolve` tolerates test mocks that return a bare unlisten fn (or
    // undefined) instead of a promise.
    Promise.resolve(
      listen<T>(event, (received) => {
        if (active) handler(received);
      }),
    )
      .then((fn) => {
        if (active) unlisten = fn;
        else fn?.();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
    // The caller's deps decide when the handler closure is replaced.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [event, ...deps]);
}
