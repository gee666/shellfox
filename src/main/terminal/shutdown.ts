export interface QuitService { ownedTerminalCount(): number; dispose(): Promise<void> }
/** Cancellation leaves every shell running. Concurrent requests share approval and disposal. */
export class TerminalQuitGuard {
  private action: Promise<boolean> | null = null;
  constructor(private readonly service: QuitService, private readonly confirm: (count: number) => boolean | Promise<boolean>) {}
  request(): Promise<boolean> {
    if (this.action) return this.action;
    this.action = Promise.resolve().then(async () => {
      const count = this.service.ownedTerminalCount();
      if (count && !await this.confirm(count)) return false;
      await this.service.dispose();
      return true;
    }).then(approved => {
      if (!approved) this.action = null;
      return approved;
    }, error => { this.action = null; throw error; });
    return this.action;
  }
}
