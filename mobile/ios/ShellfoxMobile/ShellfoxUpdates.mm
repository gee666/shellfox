#import <Foundation/Foundation.h>
#import <React/RCTBridgeModule.h>

@interface ShellfoxUpdates : NSObject <RCTBridgeModule>
@end

@implementation ShellfoxUpdates
RCT_EXPORT_MODULE(ShellfoxUpdates)

+ (BOOL)requiresMainQueueSetup { return NO; }

RCT_REMAP_METHOD(getVersion,
                 getVersionWithResolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  NSString *version = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"CFBundleShortVersionString"];
  if (version.length == 0) {
    reject(@"VERSION", @"Could not read the installed version.", nil);
    return;
  }
  resolve(version);
}

RCT_REMAP_METHOD(download,
                 downloadURL:(NSString *)url
                 sha256:(NSString *)sha256
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  reject(@"UNSUPPORTED", @"iOS updates are managed by the distribution channel, not by Shellfox.", nil);
}

RCT_REMAP_METHOD(install,
                 installURL:(NSString *)url
                 sha256:(NSString *)sha256
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  reject(@"UNSUPPORTED", @"APK updates are only supported on Android. Update iOS through its distribution channel.", nil);
}
@end
