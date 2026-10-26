/** Each view contributes polling interest without replacing another view's selection. */
export class VisibilityInterest {
  private local: string[] = [];
  private readonly remote = new Map<string, string[]>();
  private last: string[] | undefined;
  constructor(private readonly publish: (sessionIds: string[]) => void) {}
  setLocal(sessionIds: string[]): void { this.local = [...sessionIds]; this.update(); }
  setRemote(clientId: string, sessionIds: string[]): boolean {
    if (!this.remote.has(clientId) && this.remote.size >= 32) return false;
    this.remote.set(clientId, [...sessionIds]); this.update(); return true;
  }
  removeRemote(clientId: string): void { if (this.remote.delete(clientId)) this.update(); }
  private update(): void {
    const next = [...new Set([...this.local, ...[...this.remote.values()].flat()])].sort();
    if (this.last && next.length === this.last.length && next.every((id, index) => id === this.last![index])) return;
    this.last = next;
    this.publish(next);
  }
}
