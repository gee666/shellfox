#import "SFIntentQueue.h"

@implementation SFNativeIntent
@end
@implementation SFIntentQueue {
  NSMutableArray<SFNativeIntent *> *_items;
  NSUInteger _bytes;
}
- (instancetype)init { if ((self = [super init])) _items = [NSMutableArray new]; return self; }
- (NSUInteger)count { return _items.count; }
- (NSUInteger)bytes { return _bytes; }
- (SFNativeIntent *)front { return _items.firstObject; }
- (SfmStatus)append:(SFNativeIntent *)intent {
  if (intent.kind > SFIntentResize) return SFM_INVALID;
  if (intent.kind == SFIntentResize) {
    if (!intent.columns || !intent.rows || intent.columns > 512 || intent.rows > 512 ||
      uint32_t(intent.columns) * intent.rows > 65536) return SFM_INVALID;
  } else if (!intent.data.length || intent.data.length > SFM_MAX_INPUT_BYTES || intent.userInitiated > 1) return SFM_INVALID;
  NSUInteger bytes = intent.kind == SFIntentResize ? 0 : intent.data.length;
  BOOL release = intent.kind == SFIntentRelease;
  if ((release && bytes > 32768) || bytes > 163840 - _bytes) return SFM_BUSY;
  SFNativeIntent *tail = _items.lastObject;
  if (tail && intent.kind == SFIntentResize && tail.kind == SFIntentResize) {
    tail.columns = intent.columns; tail.rows = intent.rows; return SFM_OK;
  }
  if (release && tail.kind == SFIntentRelease && tail.userInitiated == intent.userInitiated &&
    tail.data.length <= 32768 - bytes) {
    NSMutableData *joined = [tail.data mutableCopy]; [joined appendData:intent.data];
    tail.data = joined; _bytes += bytes; return SFM_OK;
  }
  NSUInteger ordinary = 0, ordinaryBytes = 0;
  for (SFNativeIntent *queued in _items) if (queued.kind != SFIntentRelease) {
    ordinary++; ordinaryBytes += queued.data.length;
  }
  // Ordinary writes/resizes cannot consume the slots or bytes reserved for releases.
  if (_items.count >= 128 || (!release && (ordinary >= 64 || bytes > 131072 - ordinaryBytes))) return SFM_BUSY;
  SFNativeIntent *copy = [SFNativeIntent new];
  copy.kind = intent.kind; copy.userInitiated = intent.userInitiated;
  copy.columns = intent.columns; copy.rows = intent.rows;
  copy.data = intent.kind == SFIntentResize ? nil : [intent.data copy];
  [_items addObject:copy]; _bytes += bytes;
  return SFM_OK;
}
- (SfmStatus)appendBatch:(NSArray<SFNativeIntent *> *)intents {
  SFIntentQueue *staged = [SFIntentQueue new];
  for (SFNativeIntent *existing in _items) {
    SFNativeIntent *copy = [SFNativeIntent new];
    copy.kind = existing.kind; copy.userInitiated = existing.userInitiated;
    copy.columns = existing.columns; copy.rows = existing.rows; copy.data = [existing.data copy];
    [staged->_items addObject:copy];
  }
  staged->_bytes = _bytes;
  for (SFNativeIntent *intent in intents) {
    SfmStatus status = [staged append:intent];
    if (status != SFM_OK) return status;
  }
  _items = staged->_items; _bytes = staged->_bytes;
  return SFM_OK;
}
- (void)pop {
  SFNativeIntent *first = _items.firstObject;
  if (!first) return;
  _bytes -= first.data.length; [_items removeObjectAtIndex:0];
}
- (void)clear { [_items removeAllObjects]; _bytes = 0; }
@end
