#import "SFGoTransport.h"
#import "SFNetworkPolicy.h"
#import <Mobile/Mobile.h>
#import <UIKit/UIKit.h>
#include <atomic>

@interface SFGoRequest : NSObject {
@public
  std::atomic<bool> finished;
}
@property(nonatomic) SfmClientHandle client;
@property(nonatomic) uint64_t generation;
@property(nonatomic) uint64_t request;
@property(nonatomic) size_t limit;
@property(nonatomic, copy) NSString *key;
@property(nonatomic, strong) dispatch_source_t timer;
@property(nonatomic) UIBackgroundTaskIdentifier backgroundTask;
@end
@implementation SFGoRequest
- (instancetype)init {
  if ((self = [super init])) { finished.store(false); _backgroundTask = UIBackgroundTaskInvalid; }
  return self;
}
@end

@interface SFGoTransport ()
- (void)start:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request
         peer:(NSString *)peer path:(NSString *)path body:(NSString *)body deadline:(uint32_t)deadline limit:(size_t)limit;
- (void)cancel:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request;
@end

struct SFDispatchContext { SFGoTransport *transport; };
static void Complete(SfmClientHandle client, uint64_t generation, uint64_t request,
                     int32_t outcome, NSData *data) {
  sfm_client_complete(client, generation, request, outcome, (const uint8_t *)data.bytes, data.length);
}
static NSString *Decode(const uint8_t *bytes, size_t length) {
  return [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
}
static void Request(void *context, SfmClientHandle client, uint64_t generation, uint64_t request,
  const uint8_t *peer, size_t peerLength, const uint8_t *path, size_t pathLength,
  const uint8_t *body, size_t bodyLength, uint32_t deadline, size_t limit) {
  auto &dispatch = *static_cast<SFDispatchContext *>(context);
  @autoreleasepool {
    @try {
      // Rust borrows this context only during dispatch. All Go arguments are copied now.
      NSString *p = peerLength <= 4096 ? Decode(peer, peerLength) : nil;
      NSString *route = pathLength <= 128 ? Decode(path, pathLength) : nil;
      NSString *json = bodyLength <= SFM_CLIENT_MAX_REQUEST_BYTES ? Decode(body, bodyLength) : nil;
      if (!p || !SFNetworkPathAllowed(route) || !json || !deadline || limit > SFM_CLIENT_MAX_RESPONSE_BYTES) {
        Complete(client, generation, request, SFM_TRANSPORT_INVALID_RESPONSE, nil); return;
      }
      [dispatch.transport start:client generation:generation request:request
        peer:p path:route body:json deadline:deadline limit:limit];
    } @catch (NSException *exception) {
      Complete(client, generation, request, SFM_TRANSPORT_UNAVAILABLE, nil);
    }
  }
}
static void Cancel(void *context, SfmClientHandle client, uint64_t generation, uint64_t request) {
  auto &dispatch = *static_cast<SFDispatchContext *>(context);
  @try { [dispatch.transport cancel:client generation:generation request:request]; }
  @catch (NSException *exception) { /* No exception may cross the Rust callback. */ }
}
static NSString *Key(SfmClientHandle client, uint64_t generation, uint64_t request) {
  return [NSString stringWithFormat:@"%llu/%llu/%llu", (unsigned long long)client,
    (unsigned long long)generation, (unsigned long long)request];
}
@implementation SFGoTransport {
  NSLock *_lock;
  NSMutableDictionary<NSString *, SFGoRequest *> *_requests;
  dispatch_semaphore_t _pollSlots;
  dispatch_semaphore_t _requestSlots;
  SFGoRequestFunction _requestFunction;
}
+ (instancetype)shared {
  static SFGoTransport *transport;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ transport = [SFGoTransport new]; });
  return transport;
}
- (instancetype)init {
  return [self initWithRequestFunction:^NSString *(NSString *peer, NSString *path, NSString *body, NSError **error) {
    return MobileRequest(peer, path, body, error);
  }];
}
- (instancetype)initWithRequestFunction:(SFGoRequestFunction)request {
  if (!request) return nil;
  if ((self = [super init])) {
    _requestFunction = [request copy];
    _lock = [NSLock new]; _requests = [NSMutableDictionary new];
    _pollSlots = dispatch_semaphore_create(16); _requestSlots = dispatch_semaphore_create(12);
  }
  return self;
}
- (void)dispatchClient:(SfmClientHandle)client {
  SFDispatchContext context{self};
  sfm_client_dispatch(client, Request, Cancel, &context);
}
- (SfmStatus)closeClient:(SfmClientHandle)client {
  SFDispatchContext context{self};
  return sfm_client_close(client, Cancel, &context);
}
- (void)remove:(SFGoRequest *)request {
  [_lock lock]; [_requests removeObjectForKey:request.key]; [_lock unlock];
  if (request.timer) dispatch_source_cancel(request.timer);
  dispatch_async(dispatch_get_main_queue(), ^{
    if (request.backgroundTask != UIBackgroundTaskInvalid) {
      [UIApplication.sharedApplication endBackgroundTask:request.backgroundTask];
      request.backgroundTask = UIBackgroundTaskInvalid;
    }
  });
}
- (void)complete:(SFGoRequest *)request outcome:(int32_t)outcome data:(NSData *)data {
  if (request->finished.exchange(true)) return;
  [self remove:request];
  Complete(request.client, request.generation, request.request, outcome, data);
}
- (void)cancel:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request {
  [_lock lock]; SFGoRequest *pending = _requests[Key(client, generation, request)]; [_lock unlock];
  if (pending && !pending->finished.exchange(true)) [self remove:pending];
  // Logical cancellation cannot abort Go's socket. The physical slot stays held until return.
}
- (void)start:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request
         peer:(NSString *)peer path:(NSString *)path body:(NSString *)body deadline:(uint32_t)deadline limit:(size_t)limit {
  dispatch_semaphore_t slots = SFNetworkPathPoll(path) ? _pollSlots : _requestSlots;
  if (dispatch_semaphore_wait(slots, DISPATCH_TIME_NOW) != 0) {
    Complete(client, generation, request, SFM_TRANSPORT_UNAVAILABLE, nil); return;
  }
  SFGoRequest *pending = [SFGoRequest new]; pending.client = client;
  pending.generation = generation; pending.request = request; pending.key = Key(client, generation, request); pending.limit = limit;
  pending.timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, dispatch_get_global_queue(QOS_CLASS_UTILITY, 0));
  __weak SFGoRequest *weakRequest = pending;
  dispatch_source_set_event_handler(pending.timer, ^{
    SFGoRequest *timed = weakRequest;
    if (timed) [self complete:timed outcome:SFM_TRANSPORT_DEADLINE data:nil];
  });
  dispatch_source_set_timer(pending.timer, dispatch_time(DISPATCH_TIME_NOW, int64_t(deadline) * NSEC_PER_MSEC),
    DISPATCH_TIME_FOREVER, 10 * NSEC_PER_MSEC);
  [_lock lock]; _requests[pending.key] = pending; [_lock unlock]; dispatch_resume(pending.timer);
  if (SFNetworkPathFiniteCompletion(path)) dispatch_async(dispatch_get_main_queue(), ^{
    if (pending->finished.load()) return;
    pending.backgroundTask = [UIApplication.sharedApplication beginBackgroundTaskWithName:@"Shellfox network completion" expirationHandler:^{
      SFGoRequest *request = weakRequest;
      if (request) [self complete:request outcome:SFM_TRANSPORT_DEADLINE data:nil];
    }];
  });
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
    @autoreleasepool {
      @try {
        if (pending->finished.load()) return;
        NSError *error = nil;
        NSString *responseBody = self->_requestFunction(peer, path, body, &error);
        BOOL tooLarge = responseBody.length > limit;
        NSData *bytes = error || tooLarge ? nil : [responseBody dataUsingEncoding:NSUTF8StringEncoding];
        int32_t outcome = tooLarge ? SFM_TRANSPORT_TOO_LARGE : !bytes ? SFM_TRANSPORT_UNAVAILABLE :
          bytes.length > limit ? SFM_TRANSPORT_TOO_LARGE : SFM_TRANSPORT_OK;
        [self complete:pending outcome:outcome
          data:outcome == SFM_TRANSPORT_OK ? bytes : nil];
      } @catch (NSException *exception) {
        [self complete:pending outcome:SFM_TRANSPORT_UNAVAILABLE data:nil];
      } @finally { dispatch_semaphore_signal(slots); }
    }
  });
}
@end
