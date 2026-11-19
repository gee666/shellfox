#import "SFGoTransport.h"
#import "SFNetworkPolicy.h"
#import "SFClientProfile.h"
#import <Mobile/Mobile.h>
#import <UIKit/UIKit.h>
#include <atomic>

@implementation SFGoHTTPResponse
- (instancetype)initWithStatusCode:(long)status body:(NSString *)body {
  if ((self = [super init])) { _statusCode = status; _body = [body copy]; }
  return self;
}
@end

@interface SFGoRequest : NSObject {
@public
  std::atomic<bool> finished;
}
@property(nonatomic) SfmClientHandle client;
@property(nonatomic) uint32_t profile;
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
- (void)start:(SfmClientHandle)client profile:(uint32_t)profile generation:(uint64_t)generation request:(uint64_t)request
         peer:(NSString *)peer path:(NSString *)path body:(NSString *)body deadline:(uint32_t)deadline limit:(size_t)limit;
- (void)cancel:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request;
@end

struct SFDispatchContext { SFGoTransport *transport; uint32_t profile; };
static void Complete(SfmClientHandle client, uint32_t profile, uint64_t generation, uint64_t request,
                     int32_t outcome, uint16_t status, NSData *data) {
#ifdef SFM_ENABLE_MANAGER_V2
  if(profile==SFM_CLIENT_PROFILE_MANAGER_V2){sfm_manager_v2_complete_http(client,generation,request,outcome,status,(const uint8_t *)data.bytes,data.length);return;}
#endif
  if (profile == SFM_CLIENT_PROFILE_OBSERVER_V2)
    sfm_observer_v2_complete_http(client, generation, request, outcome, status, (const uint8_t *)data.bytes, data.length);
  else if (profile == SFM_CLIENT_PROFILE_V1)
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
        Complete(client, dispatch.profile, generation, request, SFM_TRANSPORT_INVALID_RESPONSE, 0, nil); return;
      }
      [dispatch.transport start:client profile:dispatch.profile generation:generation request:request
        peer:p path:route body:json deadline:deadline limit:limit];
    } @catch (NSException *exception) {
      Complete(client, dispatch.profile, generation, request, SFM_TRANSPORT_UNAVAILABLE, 0, nil);
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
static SFGoStatusRequestFunction StatusRequest(void) {
  return ^SFGoHTTPResponse *(NSString *peer, NSString *path, NSString *body, NSError **error) {
    // Names/types follow the pinned gomobile ObjcGen, not an invented generated header.
    MobileRequestResult *result = MobileRequestWithStatus(peer, path, body, error);
    return result ? [[SFGoHTTPResponse alloc] initWithStatusCode:[result statusCode] body:[result body]] : nil;
  };
}

@implementation SFGoTransport {
  NSLock *_lock;
  NSMutableDictionary<NSString *, SFGoRequest *> *_requests;
  dispatch_semaphore_t _pollSlots;
  dispatch_semaphore_t _requestSlots;
  SFGoRequestFunction _requestFunction;
  SFGoStatusRequestFunction _statusRequestFunction;
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
  } statusRequestFunction:StatusRequest()];
}
- (instancetype)initWithRequestFunction:(SFGoRequestFunction)request {
  return [self initWithRequestFunction:request statusRequestFunction:StatusRequest()];
}
- (instancetype)initWithRequestFunction:(SFGoRequestFunction)request statusRequestFunction:(SFGoStatusRequestFunction)statusRequest {
  if (!request || !statusRequest) return nil;
  if ((self = [super init])) {
    _requestFunction = [request copy]; _statusRequestFunction = [statusRequest copy];
    _lock = [NSLock new]; _requests = [NSMutableDictionary new];
    _pollSlots = dispatch_semaphore_create(16); _requestSlots = dispatch_semaphore_create(12);
  }
  return self;
}
- (void)dispatchClient:(SfmClientHandle)client {
  uint32_t profile = 0, capabilities = 0;
  if (sfm_client_profile(client, &profile, &capabilities) != SFM_OK) return;
  if (!shellfox::ClientAPIForProfile(profile)) return;
  SFDispatchContext context{self, profile};
  sfm_client_dispatch(client, Request, Cancel, &context);
}
- (SfmStatus)closeClient:(SfmClientHandle)client {
  SFDispatchContext context{self, 0};
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
- (void)complete:(SFGoRequest *)request outcome:(int32_t)outcome status:(uint16_t)status data:(NSData *)data {
  if (request->finished.exchange(true)) return;
  [self remove:request];
  Complete(request.client, request.profile, request.generation, request.request, outcome, status, data);
}
- (void)cancel:(SfmClientHandle)client generation:(uint64_t)generation request:(uint64_t)request {
  [_lock lock]; SFGoRequest *pending = _requests[Key(client, generation, request)]; [_lock unlock];
  if (pending && !pending->finished.exchange(true)) [self remove:pending];
  // Logical cancellation cannot abort Go's socket. The physical slot stays held until return.
}
- (void)start:(SfmClientHandle)client profile:(uint32_t)profile generation:(uint64_t)generation request:(uint64_t)request
         peer:(NSString *)peer path:(NSString *)path body:(NSString *)body deadline:(uint32_t)deadline limit:(size_t)limit {
  dispatch_semaphore_t slots = SFNetworkPathPoll(path) ? _pollSlots : _requestSlots;
  if (dispatch_semaphore_wait(slots, DISPATCH_TIME_NOW) != 0) {
    Complete(client, profile, generation, request, SFM_TRANSPORT_UNAVAILABLE, 0, nil); return;
  }
  SFGoRequest *pending = [SFGoRequest new]; pending.client = client; pending.profile = profile;
  pending.generation = generation; pending.request = request; pending.key = Key(client, generation, request); pending.limit = limit;
  pending.timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, dispatch_get_global_queue(QOS_CLASS_UTILITY, 0));
  __weak SFGoRequest *weakRequest = pending;
  dispatch_source_set_event_handler(pending.timer, ^{
    SFGoRequest *timed = weakRequest;
    if (timed) [self complete:timed outcome:SFM_TRANSPORT_DEADLINE status:0 data:nil];
  });
  dispatch_source_set_timer(pending.timer, dispatch_time(DISPATCH_TIME_NOW, int64_t(deadline) * NSEC_PER_MSEC),
    DISPATCH_TIME_FOREVER, 10 * NSEC_PER_MSEC);
  [_lock lock]; _requests[pending.key] = pending; [_lock unlock]; dispatch_resume(pending.timer);
  if (SFNetworkPathFiniteCompletion(path)) dispatch_async(dispatch_get_main_queue(), ^{
    if (pending->finished.load()) return;
    pending.backgroundTask = [UIApplication.sharedApplication beginBackgroundTaskWithName:@"Shellfox network completion" expirationHandler:^{
      SFGoRequest *request = weakRequest;
      if (request) [self complete:request outcome:SFM_TRANSPORT_DEADLINE status:0 data:nil];
    }];
  });
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
    @autoreleasepool {
      @try {
        if (pending->finished.load()) return;
        NSError *error = nil;
        NSString *responseBody = nil;
        long http = 0;
        if (profile != SFM_CLIENT_PROFILE_V1) {
          SFGoHTTPResponse *response = self->_statusRequestFunction(peer, path, body, &error);
          if (error || !response) { [self complete:pending outcome:SFM_TRANSPORT_UNAVAILABLE status:0 data:nil]; return; }
          http = response.statusCode; responseBody = response.body;
          if (http < 100 || http > 599 || ![responseBody isKindOfClass:NSString.class] ||
            ((http < 200 || http >= 300) && responseBody.length)) {
            [self complete:pending outcome:SFM_TRANSPORT_INVALID_RESPONSE status:0 data:nil]; return;
          }
        } else responseBody = self->_requestFunction(peer, path, body, &error);
        BOOL tooLarge = responseBody.length > limit;
        NSData *bytes = error || tooLarge ? nil : [responseBody dataUsingEncoding:NSUTF8StringEncoding];
        int32_t outcome = tooLarge ? SFM_TRANSPORT_TOO_LARGE : !bytes ? SFM_TRANSPORT_UNAVAILABLE :
          bytes.length > limit ? SFM_TRANSPORT_TOO_LARGE : SFM_TRANSPORT_OK;
        [self complete:pending outcome:outcome status:outcome == SFM_TRANSPORT_OK ? uint16_t(http) : 0
          data:outcome == SFM_TRANSPORT_OK ? bytes : nil];
      } @catch (NSException *exception) {
        [self complete:pending outcome:SFM_TRANSPORT_UNAVAILABLE status:0 data:nil];
      } @finally { dispatch_semaphore_signal(slots); }
    }
  });
}
@end
