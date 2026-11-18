#import <Foundation/Foundation.h>
#import <CommonCrypto/CommonDigest.h>
#import <UIKit/UIKit.h>
#import <React/RCTBridgeModule.h>
#import <React/RCTInvalidating.h>
#import <Mobile/Mobile.h>
#import "../ShellfoxRust/SFNetworkPolicy.h"

static NSURL *SFNetworkStateDirectory(NSString *name, NSError **error) {
  NSURL *base = [[NSFileManager defaultManager] URLForDirectory:NSApplicationSupportDirectory
    inDomain:NSUserDomainMask appropriateForURL:nil create:YES error:error];
  if (!base) return nil;
  NSURL *directory = [base URLByAppendingPathComponent:name isDirectory:YES];
  NSDictionary *attributes = @{NSFileProtectionKey: NSFileProtectionCompleteUntilFirstUserAuthentication};
  if (![[NSFileManager defaultManager] createDirectoryAtURL:directory withIntermediateDirectories:YES attributes:attributes error:error] ||
      ![directory setResourceValue:@YES forKey:NSURLIsExcludedFromBackupKey error:error]) return nil;
  return directory;
}

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
    NSURL *directory = SFNetworkStateDirectory(@"shellfox-tailscale", error);
    if (!directory) return nil;
    if (!MobileStart(directory.path, hostname, error)) return nil;
    return MobileStatus(error);
  } resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(startWithControlServer,
                 startHostname:(NSString *)hostname controlURL:(NSString *)controlURL
                 resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  [self submit:_control work:^id(NSError **error) {
    NSData *origin = [controlURL dataUsingEncoding:NSUTF8StringEncoding];
    if (!origin || origin.length > 2048) { *error = [NSError errorWithDomain:@"ShellfoxNetwork" code:1 userInfo:nil]; return nil; }
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(origin.bytes, (CC_LONG)origin.length, digest);
    NSMutableString *name = [NSMutableString stringWithString:@"shellfox-tailscale-loopback-"];
    for (unsigned index = 0; index < 16; ++index) [name appendFormat:@"%02x", digest[index]];
    // State isolation only. The shared Go parser validates the control origin.
    NSURL *directory = SFNetworkStateDirectory(name, error);
    if (!directory || !MobileStartWithControlServer(directory.path, hostname, controlURL, error)) return nil;
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

RCT_REMAP_METHOD(hosts,
                 hostsResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  [self submitConcurrent:NO work:^id(NSError **error) { return MobileHosts(error); }
       resolve:resolve reject:reject];
}

RCT_REMAP_METHOD(request,
                 requestPeerId:(NSString *)peerId
                 path:(NSString *)path
                 body:(NSString *)body
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject) {
  if (!SFNetworkPathAllowed(path)) { reject(@"NETWORK_ERROR", @"Network operation failed.", nil); return; }
  if (SFNetworkPathFiniteCompletion(path)) {
    // iOS grants a finite completion window, not indefinite background networking.
    dispatch_async(dispatch_get_main_queue(), ^{
      __block UIBackgroundTaskIdentifier task = UIBackgroundTaskInvalid;
      void (^finish)(void) = ^{
        dispatch_async(dispatch_get_main_queue(), ^{
          if (task != UIBackgroundTaskInvalid) {
            [[UIApplication sharedApplication] endBackgroundTask:task];
            task = UIBackgroundTaskInvalid;
          }
        });
      };
      task = [[UIApplication sharedApplication] beginBackgroundTaskWithName:@"Shellfox approval" expirationHandler:finish];
      [self submitConcurrent:SFNetworkPathPoll(path)
                       work:^id(NSError **error) { return MobileRequest(peerId, path, body, error); }
                    resolve:^(id value) { finish(); resolve(value); }
                     reject:^(NSString *code, NSString *message, NSError *error) { finish(); reject(code, message, error); }];
    });
    return;
  }
  // Long-polls must never occupy the workers used for terminal input or peers.
  [self submitConcurrent:SFNetworkPathPoll(path)
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
