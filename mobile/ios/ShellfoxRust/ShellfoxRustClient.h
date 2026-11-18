#pragma once
#import <ShellfoxRustSpecs/ShellfoxRustSpecs.h>
#import <React/RCTInvalidating.h>
#import "SFClientStore.h"

@interface ShellfoxRustClient : NativeShellfoxRustClientSpecBase <NativeShellfoxRustClientSpec, RCTInvalidating> {
@public
  SFLifetime *_lifetime;
@private
  NSLock *_registrationLock;
  BOOL _registered;
}
- (void)submit:(id (^)(SfmStatus *status))work resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject;
- (SFClientRecord *)ownedClient:(NSString *)identifier;
- (BOOL)ownsTerminal:(NSString *)identifier;
@end

