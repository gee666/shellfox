import type { TerminalApi } from '../../../src/shared/contracts';
import { responseSchemas } from '../../../src/shared/schemas';

// Keep ordering and failed cleanup across a rapid unmount/remount of the same tab.
interface DeliveryState { queue?: Promise<unknown>; generation?: string }
const clients = new WeakMap<object, Map<string, DeliveryState>>();
function state(api: object, tabId: string): DeliveryState {
  let tabs = clients.get(api);
  if (!tabs) { tabs = new Map(); clients.set(api, tabs); }
  let delivery = tabs.get(tabId);
  if (!delivery) { delivery = {}; tabs.set(tabId, delivery); }
  return delivery;
}
export function trackAttachment(api: object, tabId: string, generation: string): void {
  state(api, tabId).generation = generation;
}
/** Call inside deliveryOperation. A failed detach must be retried before the next attach. */
export async function detachAttachment(api: Pick<TerminalApi, 'detachTerminal'>, tabId: string): Promise<void> {
  const delivery = state(api, tabId), generation = delivery.generation;
  if (!generation) return;
  const result = responseSchemas.detachTerminal.parse(await api.detachTerminal({ tabId, generation }));
  if (!result.ok && result.error.code !== 'TARGET_LOST' && result.error.code !== 'NOT_FOUND') throw new Error(result.error.message);
  if (delivery.generation === generation) delete delivery.generation;
}
export function deliveryOperation<T>(api: object, tabId: string, operation: () => Promise<T>): Promise<T> {
  const delivery = state(api, tabId);
  const task = Promise.resolve(delivery.queue).catch(() => undefined).then(operation);
  delivery.queue = task;
  const cleanup = () => {
    if (delivery.queue !== task) return;
    delete delivery.queue;
    if (!delivery.generation) clients.get(api)?.delete(tabId);
  };
  void task.then(cleanup, cleanup);
  return task;
}
