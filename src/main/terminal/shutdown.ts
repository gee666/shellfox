export interface QuitService { ownedTerminalCount(): number; dispose(): Promise<void> }
/** Native confirmation is independent of the sandboxed renderer. Cancellation changes nothing. */
export class TerminalQuitGuard {
  private action: Promise<boolean> | null = null;
  constructor(private readonly service: QuitService, private readonly confirm: (count: number) => boolean) {}
  request(): Promise<boolean> {
    if (this.action) return this.action;
    const count = this.service.ownedTerminalCount();
    if (count && !this.confirm(count)) return Promise.resolve(false);
    this.action = this.service.dispose().then(() => true).catch(error => { this.action = null; throw error; });
    return this.action;
  }
}
