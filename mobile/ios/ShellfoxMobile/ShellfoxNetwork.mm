#import <Foundation/Foundation.h>
#import <React/RCTBridgeModule.h>
#import <React/RCTInvalidating.h>
#import <Mobile/Mobile.h>

@interface ShellfoxNetwork : NSObject <RCTBridgeModule, RCTInvalidating>
@end

@implementation ShellfoxNetwork {
  NSOperationQueue *_control;
  dispatch_semaphore_t _requestSlots;
  dispatch_semaphore_t _pollSlots;
  NSLock *_lock;
  NSUInteger _pending;
  BOOL _invalidated;
}

RCT_EXPORT_MODULE(ShellfoxNetwork)

+ (BOOL)requiresMainQueueSetup { return NO; }

- (instancetype)init {
  if ((self = [super init])) {
    _lock = [NSLock new];
    _control = [NSOperationQueue new];
    _control.maxConcurrentOperationCount = 1;
    _control.qualityOfService = NSQualityOfServiceUserInitiated;
    _requestSlots = dispatch_semaphore_create(12);
    _pollSlots = dispatch_semaphore_create(64);
  }
  return self;
}

- (void)submit:(NSOperationQueue *)queue
          work:(id (^)(NSError **))work
       resolve:(RCTPromiseResolveBlock)resolve
        reject:(RCTPromiseRejectBlock)reject {
  [_lock lock];
  BOOL available = !_invalidated && _pending < 128;
  if (available) _pending++;
  [_lock unlock];
  if (!available) {
    reject(@"NETWORK_BUSY", @"Network module is unavailable or busy.", nil);
    return;
  }
  [queue addOperationWithBlock:^{
    @autoreleasepool {
      [self->_lock lock];
      BOOL stopped = self->_invalidated;
      [self->_lock unlock];
      NSError *error = nil;
      id result = stopped ? nil : work(&error);
      // SDK errors can contain credentials, login URLs, paths or remote data.
      if (stopped || error) reject(@"NETWORK_ERROR", @"Network operation failed.", nil);
      else resolve(result ?: [NSNull null]);
      [self->_lock lock];
      self->_pending--;
      [self->_lock unlock];
    }
  }];
}

- (void)submitConcurrent:(BOOL)poll
                    work:(id (^)(NSError **))work
                 resolve:(RCTPromiseResolveBlock)resolve
                  reject:(RCTPromiseRejectBlock)reject {
  dispatch_semaphore_t slots = poll ? _pollSlots : _requestSlots;
  // Admission happens now, not after a queued JS operation has timed out.
  if (dispatch_semaphore_wait(slots, DISPATCH_TIME_NOW) != 0) {
    reject(@"NETWORK_BUSY", @"Too many pending network operations.", nil);
    return;
  }
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
    @autoreleasepool {
      [self->_lock lock];
      BOOL stopped = self->_invalidated;
      [self->_lock unlock];
      NSError *error = nil;
      id result = stopped ? nil : work(&error);
      if (stopped || error) reject(@"NETWORK_ERROR", @"Network operation failed.", nil);
      else resolve(result ?: [NSNull null]);
      dispatch_semaphore_signal(slots);
    }
  });
}

RCT_REMAP_METHOD(start,
                 startHostname:(NSString *)hostname
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) {
    NSURL *base = [[NSFileManager defaultManager] URLForDirectory:NSApplicationSupportDirectory
                          inDomain:NSUserDomainMask appropriateForURL:nil create:YES error:error];
    if (!base) return nil;
    NSURL *directory = [base URLByAppendingPathComponent:@"shellfox-tailscale" isDirectory:YES];
    NSDictionary *attributes = @{NSFileProtectionKey: NSFileProtectionCompleteUntilFirstUserAuthentication};
    if (![[NSFileManager defaultManager] createDirectoryAtURL:directory withIntermediateDirectories:YES
                                              attributes:attributes error:error]) return nil;
    if (![directory setResourceValue:@YES forKey:NSURLIsExcludedFromBackupKey error:error]) return nil;
    if (!MobileStart(directory.path, hostname, error)) return nil;
    return MobileStatus(error);
  } resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(status,
                 statusResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) { return MobileStatus(error); }
       resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(login,
                 loginAuthKey:(NSString *)authKey
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) {
    if (!MobileLogin(authKey, error)) return nil;
    return MobileStatus(error);
  } resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(peers,
                 peersResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submitConcurrent:NO work:^id(NSError **error) { return MobilePeers(error); }
       resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(request,
                 requestPeerId:(NSString *)peerId
                 path:(NSString *)path
                 body:(NSString *)body
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  // Long-polls must never occupy the workers used for terminal input or peers.
  [self submitConcurrent:[path isEqualToString:@"/v1/events"]
                    work:^id(NSError **error) { return MobileRequest(peerId, path, body, error); }
       resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(stop,
                 stopResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) { MobileStop(error); return nil; }
       resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(logout,
                 logoutResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) { MobileLogout(error); return nil; }
       resolve:resolve reject:reject];
}

- (void)invalidate {
  [_lock lock];
  _invalidated = YES;
  [_lock unlock];
  // iOS can suspend this foreground-only node. Do not claim VPN/background
  // execution or stop on resignActive, which also occurs during browser login.
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
    NSError *error = nil;
    MobileStop(&error);
  });
}
@end
