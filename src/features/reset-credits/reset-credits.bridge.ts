import { useResetCreditStore } from './reset-credits.store';
import { isResetCreditState } from './reset-credits.types';

interface BridgeEvent {
  payload: unknown;
}

export interface ResetCreditBridgeApi {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  listen(event: string, handler: (event: BridgeEvent) => void): Promise<() => void>;
}

export interface ResetCreditBridge {
  refresh(reason: string): Promise<void>;
  stop(): Promise<void>;
}

export async function startResetCreditBridge(
  api: ResetCreditBridgeApi,
): Promise<ResetCreditBridge> {
  let unlisten: (() => void) | null = null;
  let stopped = false;
  let updateSeen = false;
  try {
    unlisten = await api.listen('reset-credits://updated', ({ payload }) => {
      if (!stopped && isResetCreditState(payload)) {
        updateSeen = true;
        useResetCreditStore.getState().applySnapshot(payload);
      }
    });

    const initial = await api.invoke('get_reset_credits');
    if (!stopped && !updateSeen && isResetCreditState(initial)) {
      useResetCreditStore.getState().applySnapshot(initial);
    }
  } catch (error) {
    stopped = true;
    unlisten?.();
    useResetCreditStore.getState().clear();
    throw error;
  }

  return {
    async refresh(reason) {
      await api.invoke('refresh_reset_credits', { reason });
    },
    async stop() {
      if (stopped) {
        return;
      }
      stopped = true;
      unlisten?.();
    },
  };
}
