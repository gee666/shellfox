#import "SFOwner.h"
#import "SFClientStore.h"
#include <deque>
#include <chrono>
#include <condition_variable>

BOOL SFParseId(NSString *value, uint64_t *result) {
  if (![value isKindOfClass:NSString.class] || !value.length || value.length > 20 || [value characterAtIndex:0] == '0') return NO;
  uint64_t parsed = 0;
  for (NSUInteger i = 0; i < value.length; ++i) {
    unichar ch = [value characterAtIndex:i];
    if (ch < '0' || ch > '9' || parsed > (UINT64_MAX - (ch - '0')) / 10) return NO;
    parsed = parsed * 10 + ch - '0';
  }
  *result = parsed; return YES;
}
NSString *SFDecimal(uint64_t value) { return [NSString stringWithFormat:@"%llu", (unsigned long long)value]; }

@interface SFTerminal ()
@property(nonatomic, copy, readwrite) NSString *identifier;
@property(nonatomic, readwrite) SfmHandle handle;
@end
@implementation SFTerminal
- (instancetype)init {
  if ((self = [super init])) _pendingIntents = [SFIntentQueue new];
  return self;
}
@end
@implementation SFLifetime
- (instancetype)init {
  if ((self = [super init])) {
    invalidated.store(false); _terminals = [NSMutableSet new]; _clients = [NSMutableSet new];
  }
  return self;
}
@end

static void PumpError(void *context, SfmStatus status) {
  SFTerminal *terminal = (__bridge SFTerminal *)context;
  terminal.visible = NO;
  // C callbacks cannot reenter the ABI. Delivery happens after pump returns.
  terminal.dirty = NO;
  terminal.mailbox->failure.store(status);
}
static void CopyFrame(void *context, const SfmFrame *source) {
  if (!source) return;
  SFTerminal *terminal = (__bridge SFTerminal *)context;
  if (!terminal.mailbox->active.load()) return;
  try {
    auto frame = shellfox::Frame::copy(*source);
    if (!frame) terminal.mailbox->failure.store(SFM_ENGINE);
    else if (terminal.mailbox->active.load()) {
      terminal.columns = frame->cols; terminal.rows = frame->rows;
      terminal.mailbox->put(std::move(frame));
    }
  } catch (...) { terminal.mailbox->failure.store(SFM_PANIC); }
}
@implementation SFOwner {
  NSThread *_thread;
  std::mutex _mutex;
  std::condition_variable _wake;
  std::deque<SFOwnerWork> _pending;
  NSMutableDictionary<NSString *, SFTerminal *> *_terminals;
  NSMutableArray<SFLifetime *> *_lifetimes;
}
+ (instancetype)shared {
  static SFOwner *owner;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ owner = [SFOwner new]; });
  return owner;
}
- (instancetype)init {
  if ((self = [super init])) {
    _terminals = [NSMutableDictionary new]; _lifetimes = [NSMutableArray new];
    _thread = [[NSThread alloc] initWithTarget:self selector:@selector(run) object:nil];
    _thread.name = @"Shellfox Rust owner";
    _thread.qualityOfService = NSQualityOfServiceUserInteractive;
    [_thread start];
  }
  return self;
}
- (BOOL)isOwnerThread { return NSThread.currentThread == _thread; }
- (BOOL)registerLifetime:(SFLifetime *)lifetime {
  std::lock_guard<std::mutex> guard(_mutex);
  BOOL accepted = _lifetimes.count < 16;
  if (accepted) { [_lifetimes addObject:lifetime]; _wake.notify_one(); }
  return accepted;
}
- (void)retireLifetimes {
  NSArray<SFLifetime *> *lifetimes;
  { std::lock_guard<std::mutex> guard(_mutex); lifetimes = _lifetimes.copy; }
  for (SFLifetime *lifetime in lifetimes) {
    if (!lifetime->invalidated.load()) continue;
    for (NSString *identifier in lifetime.terminals.allObjects) {
      SFTerminal *terminal = _terminals[identifier];
      if (!terminal) { [lifetime.terminals removeObject:identifier]; continue; }
      terminal.retiring = YES;
      [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
      terminal.mailbox->ready.store(false);
    }
    for (NSString *identifier in lifetime.clients.allObjects) {
      if ([[SFClientStore shared] client:identifier]) [[SFClientStore shared] close:identifier];
      else [lifetime.clients removeObject:identifier]; // Actual native close removed the owner record.
    }
    if (lifetime.clients.count) continue;
    for (NSString *identifier in lifetime.terminals.allObjects) {
      if ([self close:identifier] == SFM_OK) [lifetime.terminals removeObject:identifier];
    }
    if (!lifetime.terminals.count) {
      std::lock_guard<std::mutex> guard(_mutex); [_lifetimes removeObject:lifetime];
    }
  }
}
- (BOOL)enqueue:(SFOwnerWork)work {
  std::lock_guard<std::mutex> guard(_mutex);
  BOOL available = _pending.size() < 128;
  if (available) { _pending.push_back([work copy]); _wake.notify_one(); }
  return available;
}
- (void)run {
  NSTimeInterval nextTick = 0;
  for (;;) {
    @autoreleasepool {
      std::deque<SFOwnerWork> batch;
      {
        std::unique_lock<std::mutex> lock(_mutex);
        double delay = nextTick - NSProcessInfo.processInfo.systemUptime;
        if (_pending.empty() && delay > 0) _wake.wait_for(lock, std::chrono::duration<double>(delay));
        for (unsigned i = 0; i < 16 && !_pending.empty(); ++i) {
          batch.push_back(std::move(_pending.front())); _pending.pop_front();
        }
      }
      for (SFOwnerWork work : batch) work();
      [self retireLifetimes];
      NSTimeInterval now = NSProcessInfo.processInfo.systemUptime;
      if (now < nextTick) continue;
      nextTick = now + 1.0 / 60;
      [[SFClientStore shared] tick];
      for (SFTerminal *terminal in _terminals.allValues) if (!terminal.retiring) [self retryIntents:terminal];
      for (SFTerminal *terminal in _terminals.allValues) {
        if (terminal.hostEncoding && terminal.sink) {
          uint8_t ready = 0;
          if ([terminal.sink recoveryReady:terminal.identifier ready:&ready] != SFM_OK || !ready) {
            if (!terminal.recovering) [terminal.pendingIntents clear];
            terminal.recovering = YES; terminal.recoveryInstalled = YES;
            terminal.mailbox->ready.store(false);
          }
        }
        BOOL presenting = !terminal.retiring && terminal.visible && terminal.mailbox->active.load();
        // Detach waits for the parser, including when UIKit has removed its view.
        // Keep consuming accepted output without copying a frame for an offscreen terminal.
        if ((!presenting && !terminal.sink && !terminal.retiring) ||
          (terminal.recovering && !terminal.recoveryInstalled)) continue;
        if (presenting && terminal.viewportUnaccepted && !terminal.recovering && !terminal.readOnly)
          [self resize:terminal.identifier columns:terminal.viewportColumns rows:terminal.viewportRows];
        SfmProgress before = {};
        SfmStatus status = sfm_terminal_progress(terminal.handle, &before);
        if (status == SFM_OK && (before.queued_chunks || (presenting && terminal.dirty))) {
          status = sfm_terminal_pump(terminal.handle, PumpError, (__bridge void *)terminal);
          SfmProgress after = {};
          if (status == SFM_OK) status = sfm_terminal_progress(terminal.handle, &after);
          if (status == SFM_OK) {
            if (presenting && (after.consumed_sequence != terminal.delivered || terminal.dirty))
              status = sfm_terminal_frame(terminal.handle, CopyFrame, (__bridge void *)terminal);
            terminal.delivered = after.consumed_sequence;
            terminal.dirty = !presenting;
          }
        }
        // Retry credit even on idle ticks. BUSY accepts nothing and ACK can fail asynchronously.
        if (status == SFM_OK && terminal.sink) {
          SfmStatus credit = [terminal.sink consumedTerminal:terminal.identifier sequence:terminal.delivered];
          uint8_t ready = 0;
          if (terminal.recovering && terminal.recoveryInstalled &&
            (credit == SFM_OK || credit == SFM_BUSY || credit == SFM_CLIENT_EMPTY) &&
            [terminal.sink recoveryReady:terminal.identifier ready:&ready] == SFM_OK && ready) {
            terminal.recovering = NO; terminal.recoveryInstalled = NO;
            terminal.mailbox->ready.store(true);
            if (presenting) [self resize:terminal.identifier columns:terminal.viewportColumns rows:terminal.viewportRows];
          }
        }
        if (status != SFM_OK) {
          terminal.visible = NO;
          [terminal.sink terminalFailed:terminal.identifier status:status];
        }
      }
    }
  }
}
- (SfmStatus)create:(uint16_t)cols rows:(uint16_t)rows result:(SFTerminal **)result {
  if (![self isOwnerThread]) return SFM_WRONG_THREAD;
  if (sfm_abi_version() != SFM_ABI_VERSION) return SFM_INVALID;
  SfmHandle handle = 0;
  SfmStatus status = sfm_terminal_create(cols, rows, &handle);
  if (status != SFM_OK) return status;
  SFTerminal *terminal = [SFTerminal new];
  terminal.handle = handle;
  terminal.identifier = [NSString stringWithFormat:@"%llu", (unsigned long long)handle];
  terminal.mailbox = std::make_shared<shellfox::Mailbox>();
  terminal.dirty = YES;
  terminal.columns = cols; terminal.rows = rows;
  terminal.viewportColumns = cols; terminal.viewportRows = rows;
  _terminals[terminal.identifier] = terminal;
  *result = terminal;
  return SFM_OK;
}
- (SFTerminal *)terminal:(NSString *)identifier {
  return [self isOwnerThread] ? _terminals[identifier] : nil;
}
- (SfmStatus)close:(NSString *)identifier {
  if (![self isOwnerThread]) return SFM_WRONG_THREAD;
  SFTerminal *terminal = _terminals[identifier];
  if (!terminal) return SFM_STALE;
  SfmProgress progress{};
  if (terminal.sink || terminal.closeAttempted) return SFM_BUSY;
  SfmStatus status = sfm_terminal_progress(terminal.handle, &progress);
  if (status != SFM_OK) return status;
  if (progress.accepted_sequence != progress.consumed_sequence || progress.queued_bytes || progress.queued_chunks) return SFM_BUSY;
  terminal.closeAttempted = YES;
  status = sfm_terminal_close(terminal.handle);
  if (status == SFM_OK) {
    terminal.visible = NO; [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
    terminal.mailbox->active.store(false); terminal.mailbox->ready.store(false); terminal.mailbox->closed.store(true);
    [_terminals removeObjectForKey:identifier];
  } else terminal.mailbox->failure.store(status);
  return status;
}
- (SfmStatus)dispatchIntent:(SFNativeIntent *)intent terminal:(SFTerminal *)terminal {
  if (terminal.retiring || !terminal.sink) return SFM_STALE;
  if (terminal.deliveryOnly) return SFM_INVALID;
  if (terminal.recovering) return SFM_BUSY;
  if (intent.kind == SFIntentResize) return terminal.readOnly ? SFM_OK :
    [terminal.sink resizeTerminal:terminal.identifier columns:intent.columns rows:intent.rows];
  if (intent.kind != SFIntentWrite && intent.kind != SFIntentRelease) return SFM_INVALID;
  if (terminal.readOnly && intent.kind != SFIntentRelease) return SFM_STALE;
  NSData *data = intent.data;
  if (!data.length || data.length > SFM_MAX_INPUT_BYTES) return SFM_INVALID;
  if (intent.nativeJson) return [terminal.sink sendCanonicalInput:data terminal:terminal.identifier];
  return [terminal.sink sendInput:data userInitiated:intent.userInitiated terminal:terminal.identifier];
}
- (SfmStatus)submitIntent:(SFNativeIntent *)intent terminal:(SFTerminal *)terminal {
  if (terminal.pendingIntents.count) return [terminal.pendingIntents append:intent];
  SfmStatus status = [self dispatchIntent:intent terminal:terminal];
  return status == SFM_BUSY ? [terminal.pendingIntents append:intent] : status;
}
- (SfmStatus)retryIntents:(SFTerminal *)terminal {
  for (unsigned count = 0; terminal.pendingIntents.count && count < 16; ++count) {
    SfmStatus status = [self dispatchIntent:terminal.pendingIntents.front terminal:terminal];
    if (status == SFM_BUSY) return SFM_OK; // Retained natively; Rust accepted nothing. Never retry after SFM_OK.
    if (status != SFM_OK) {
      [terminal.pendingIntents clear]; terminal.mailbox->failure.store(status); return status;
    }
    [terminal.pendingIntents pop];
  }
  return SFM_OK;
}
- (SfmStatus)drainBeforeCommand:(SFTerminal *)terminal {
  // Snapshot the FIFO ticket. New transport submissions cannot extend this barrier.
  SfmProgress progress = {};
  SfmStatus status = sfm_terminal_progress(terminal.handle, &progress);
  if (status != SFM_OK) return status;
  uint64_t target = progress.accepted_sequence;
  for (unsigned count = 0; progress.consumed_sequence < target && count < SFM_MAX_QUEUE_CHUNKS; ++count) {
    status = sfm_terminal_pump(terminal.handle, PumpError, (__bridge void *)terminal);
    if (status != SFM_OK) return status;
    terminal.dirty = YES;
    status = sfm_terminal_progress(terminal.handle, &progress);
    if (status != SFM_OK) return status;
  }
  return progress.consumed_sequence >= target ? SFM_OK : SFM_BUSY;
}
@end
