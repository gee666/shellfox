#import "SFOwner.h"

@interface SFOwner (InputHelpers)
- (SfmStatus)drainBeforeCommand:(SFTerminal *)terminal;
- (SfmStatus)submitIntent:(SFNativeIntent *)intent terminal:(SFTerminal *)terminal;
- (SfmStatus)retryIntents:(SFTerminal *)terminal;
@end

struct SFEncodedOutput {
  NSMutableArray<SFNativeIntent *> *packets;
  SFIntentKind kind = SFIntentWrite;
  size_t bytes = 0;
  bool failed = false;
};
static void CopyInputEx(void *context, const uint8_t *bytes, size_t length, uint8_t hint) {
  auto &output = *static_cast<SFEncodedOutput *>(context);
  if (hint > 1 || length > SFM_MAX_INPUT_BYTES || output.bytes > SFM_MAX_INPUT_BYTES - length) {
    output.failed = true; return;
  }
  @try {
    SFNativeIntent *last = output.packets.lastObject;
    if (last && last.kind == output.kind && last.userInitiated == hint) {
      NSMutableData *joined = [last.data mutableCopy]; [joined appendBytes:bytes length:length]; last.data = joined;
    } else {
      SFNativeIntent *packet = [SFNativeIntent new]; packet.kind = output.kind; packet.userInitiated = hint;
      packet.data = [NSData dataWithBytes:bytes length:length]; [output.packets addObject:packet];
    }
    output.bytes += length;
  } @catch (NSException *exception) { output.failed = true; }
}

@implementation SFOwner (Input)
- (SfmStatus)resize:(NSString *)identifier columns:(uint16_t)cols rows:(uint16_t)rows {
  if (![self isOwnerThread]) return SFM_WRONG_THREAD;
  SFTerminal *terminal = [self terminal:identifier];
  if (!terminal) return SFM_STALE;
  // An unmeasured viewport never resizes: fitting a recovered screen to the
  // placeholder would push rows above a parked cursor into scrollback for good.
  if (!cols || !rows) return SFM_OK;
  terminal.viewportColumns = cols; terminal.viewportRows = rows;
  if (terminal.recovering) return SFM_OK;
  BOOL changed = terminal.columns != cols || terminal.rows != rows;
  // A read-only view resizes only its own engine, so compare with the host's grid too.
  BOOL hostStale = !terminal.readOnly && terminal.sink && (terminal.hostColumns != cols || terminal.hostRows != rows);
  if (!changed && !hostStale && !terminal.viewportUnaccepted) return SFM_OK;
  SfmStatus status = [self drainBeforeCommand:terminal];
  if (status == SFM_OK && changed) status = sfm_terminal_resize(terminal.handle, cols, rows);
  if (status == SFM_OK) {
    terminal.dirty = YES;
    terminal.columns = cols; terminal.rows = rows;
    if (!terminal.readOnly && terminal.sink) {
      SFNativeIntent *intent = [SFNativeIntent new]; intent.kind = SFIntentResize;
      intent.columns = cols; intent.rows = rows;
      status = [self submitIntent:intent terminal:terminal];
      if (status == SFM_OK) { terminal.hostColumns = cols; terminal.hostRows = rows; }
    }
    terminal.viewportUnaccepted = status == SFM_BUSY;
  }
  return status;
}
static BOOL SFIsRelease(NSDictionary *event) {
  return [event[@"type"] isEqual:@"key"] && [event[@"event"][@"action"] isEqual:@"release"];
}
- (SfmStatus)input:(NSString *)identifier event:(NSDictionary *)event {
  return [self input:identifier events:@[event]];
}
- (SfmStatus)input:(NSString *)identifier events:(NSArray<NSDictionary *> *)events {
  if (![self isOwnerThread]) return SFM_WRONG_THREAD;
  if (!events.count || events.count > 128) return SFM_INVALID;
  SFTerminal *terminal = [self terminal:identifier];
  if (!terminal) return SFM_STALE;
  BOOL release = YES;
  for (NSDictionary *event in events) {
    if (![event isKindOfClass:NSDictionary.class] ||
        ([event[@"type"] isEqual:@"key"] && ![event[@"event"] isKindOfClass:NSDictionary.class])) return SFM_INVALID;
    if (!SFIsRelease(event)) release = NO;
  }
  // An editor reset only clears Rust's record of the keyboard editor.
  BOOL preedit = events.count == 1 && [events.firstObject[@"type"] isEqual:@"editorReset"];
  if (terminal.recovering && release) return SFM_OK; // Old physical state was fenced; never replay its release into the new lease.
  if (!preedit && (terminal.recovering || (terminal.readOnly && !release))) return SFM_BUSY;
  SfmStatus status = preedit ? SFM_OK : [self drainBeforeCommand:terminal];
  if (status != SFM_OK) return status;
  SFEncodedOutput encoded{[NSMutableArray new]};
  for (NSDictionary *event in events) {
    NSData *json = [NSJSONSerialization dataWithJSONObject:event options:0 error:nil];
    if (!json || json.length > SFM_MAX_INPUT_BYTES) return SFM_INVALID;
    // This kind reserves release capacity only. Rust alone supplies settlement/activity metadata.
    encoded.kind = SFIsRelease(event) ? SFIntentRelease : SFIntentWrite;
    status = sfm_terminal_input_ex(terminal.handle, (const uint8_t *)json.bytes, json.length, CopyInputEx, &encoded);
    if (status != SFM_OK) return status;
    if (encoded.failed) return SFM_INVALID;
  }
  if (preedit && encoded.bytes) return SFM_INVALID;
  terminal.dirty = YES;
  if (encoded.packets.count == 1) return [self submitIntent:encoded.packets.firstObject terminal:terminal];
  if (encoded.packets.count) {
    status = [terminal.pendingIntents appendBatch:encoded.packets];
    if (status == SFM_OK) status = [self retryIntents:terminal];
  }
  return status;
}
@end
