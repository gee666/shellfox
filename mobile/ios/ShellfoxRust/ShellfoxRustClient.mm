#import "ShellfoxRustClient.h"
#include <cmath>


@implementation ShellfoxRustClient
RCT_EXPORT_MODULE(NativeShellfoxRustClient)
+ (BOOL)requiresMainQueueSetup { return NO; }
- (instancetype)init {
  if ((self = [super init])) {
    _lifetime = [SFLifetime new]; _registrationLock = [NSLock new];
    // RN providers instantiate a prototype before the manager creates a module instance.
    // Do not start an owner or consume its lifetime slots for that unused prototype.
  }
  return self;
}
- (void)dealloc { _lifetime->invalidated.store(true); }
- (void)invalidate { _lifetime->invalidated.store(true); }
- (NSNumber *)abiVersion { return @(sfm_abi_version()); }
- (std::shared_ptr<facebook::react::TurboModule>)getTurboModule:(const facebook::react::ObjCTurboModule::InitParams &)params {
  return std::make_shared<facebook::react::NativeShellfoxRustClientSpecJSI>(params);
}
- (BOOL)ownsTerminal:(NSString *)identifier {
  uint64_t parsed = 0;
  return SFParseId(identifier, &parsed) && [_lifetime.terminals containsObject:identifier];
}
- (SFClientRecord *)ownedClient:(NSString *)identifier {
  uint64_t parsed = 0;
  SFClientRecord *client = SFParseId(identifier, &parsed) && [_lifetime.clients containsObject:identifier] ?
    [[SFClientStore shared] client:identifier] : nil;
  return client.closing ? nil : client;
}
- (void)submit:(id (^)(SfmStatus *))work resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if (_lifetime->invalidated.load()) { reject(@"RUST_CLOSED", @"Native client is closed.", nil); return; }
  [_registrationLock lock];
  if (!_registered) _registered = [[SFOwner shared] registerLifetime:_lifetime];
  BOOL registered = _registered;
  [_registrationLock unlock];
  if (!registered) { reject(@"RUST_BUSY", @"Native lifetime registry is full.", nil); return; }
  BOOL accepted = [[SFOwner shared] enqueue:^{
    if (self->_lifetime->invalidated.load()) { reject(@"RUST_CLOSED", @"Native client is closed.", nil); return; }
    SfmStatus status = SFM_OK;
    id result = work(&status);
    if (status == SFM_OK) resolve(result ?: NSNull.null);
    else reject([NSString stringWithFormat:@"RUST_%d", status], @"Native operation failed.", nil);
  }];
  if (!accepted) reject(@"RUST_BUSY", @"Native queue is full.", nil);
}
- (void)createTerminal:(double)cols rows:(double)rows resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if (!std::isfinite(cols) || !std::isfinite(rows) || cols != floor(cols) || rows != floor(rows) ||
    cols < 1 || cols > 512 || rows < 1 || rows > 512 || cols * rows > 65536) {
    reject(@"RUST_INVALID", @"Invalid terminal dimensions.", nil); return;
  }
  [self submit:^id(SfmStatus *status) {
    SFTerminal *terminal = nil;
    *status = [[SFOwner shared] create:(uint16_t)cols rows:(uint16_t)rows result:&terminal];
    if (terminal) [self->_lifetime.terminals addObject:terminal.identifier];
    return terminal.identifier;
  } resolve:resolve reject:reject];
}
- (void)closeTerminal:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    SFTerminal *original = [[SFOwner shared] terminal:identifier];
    if (![self ownsTerminal:identifier] || !original) { *status = SFM_STALE; return nil; }
    NSString *bound = nil;
    for (NSString *clientId in self->_lifetime.clients) {
      SFClientRecord *client = [[SFClientStore shared] client:clientId];
      if (client && [[SFClientStore shared] isBound:identifier client:client]) { bound = clientId; break; }
    }
    SFClientDeferredWork work = ^id(SfmStatus *actual) {
      if (self->_lifetime->invalidated.load() || [[SFOwner shared] terminal:identifier] != original) { *actual = SFM_STALE; return nil; }
      if (bound) *actual = [[SFClientStore shared] unbind:bound terminal:identifier];
      if (*actual == SFM_OK || *actual == SFM_STALE) *actual = [[SFOwner shared] close:identifier];
      if (*actual == SFM_OK || *actual == SFM_STALE) [self->_lifetime.terminals removeObject:identifier];
      return nil;
    };
    SFClientDeferredDone done = ^(id value, SfmStatus actual) {
      if (actual == SFM_OK) resolve(NSNull.null);
      else reject(@"RUST_CLOSE", @"Native terminal close failed.", nil);
    };
    if (bound) [[SFClientStore shared] whenDrained:bound admission:SFClientAdmissionV1RetireView work:work done:done];
    else { SfmStatus actual = SFM_OK; id value = work(&actual); done(value, actual); }
    return nil;
  } resolve:^(id ignored) {} reject:reject];
}
- (void)progress:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    if (![self ownsTerminal:identifier]) { *status = SFM_STALE; return nil; }
    SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
    SfmProgress progress = {};
    *status = terminal ? sfm_terminal_progress(terminal.handle, &progress) : SFM_STALE;
    if (*status != SFM_OK) return nil;
    NSDictionary *value = @{@"acceptedSequence":SFDecimal(progress.accepted_sequence),
      @"consumedSequence":SFDecimal(progress.consumed_sequence), @"queuedBytes":@(progress.queued_bytes),
      @"queuedChunks":@(progress.queued_chunks)};
    NSData *json = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    if (json.length > 1024) { *status = SFM_INVALID; return nil; }
    return [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
  } resolve:resolve reject:reject];
}
- (void)terminalInput:(NSString *)identifier json:(NSString *)json resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [json dataUsingEncoding:NSUTF8StringEncoding];
  if (!bytes || bytes.length > SFM_MAX_INPUT_BYTES) { reject(@"RUST_INVALID", @"Invalid input event.", nil); return; }
  id input = [NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];
  if (![input isKindOfClass:NSDictionary.class]) { reject(@"RUST_INVALID", @"Invalid input event.", nil); return; }
  [self submit:^id(SfmStatus *status) {
    if (![self ownsTerminal:identifier]) { *status = SFM_STALE; return nil; }
    *status = [[SFOwner shared] input:identifier event:input];
    return nil;
  } resolve:resolve reject:reject];
}
- (void)configureTerminal:(NSString *)identifier json:(NSString *)json resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [json dataUsingEncoding:NSUTF8StringEncoding];
  if (!bytes || bytes.length > 16384) { reject(@"RUST_INVALID", @"Invalid terminal configuration.", nil); return; }
  [self submit:^id(SfmStatus *status) {
    SFTerminal *terminal = [self ownsTerminal:identifier] ? [[SFOwner shared] terminal:identifier] : nil;
    if (!terminal) { *status = SFM_STALE; return nil; }
    *status = sfm_terminal_configure(terminal.handle, (const uint8_t *)bytes.bytes, bytes.length);
    if (*status != SFM_OK) return nil;
    // Rust has validated types, bounds and every color before renderer config changes.
    NSDictionary *config = [NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];
    auto hex = ^uint32_t(NSString *key, uint32_t fallback) {
      id value = config[key];
      if (![value isKindOfClass:NSString.class]) return fallback;
      unsigned int rgb = 0;
      NSScanner *scanner = [NSScanner scannerWithString:[value substringFromIndex:1]];
      return [scanner scanHexInt:&rgb] ? rgb : fallback;
    };
    shellfox::Appearance appearance;
    appearance.foreground = hex(@"foreground", appearance.foreground);
    appearance.background = hex(@"background", appearance.background);
    appearance.cursor = hex(@"cursor", appearance.foreground);
    appearance.selectionBackground = hex(@"selectionBackground", appearance.selectionBackground);
    appearance.selectionForeground = hex(@"selectionForeground", appearance.selectionForeground);
    terminal.mailbox->setAppearance(appearance); terminal.dirty = YES;
    return nil;
  } resolve:resolve reject:reject];
}
- (void)createClient:(NSString *)peerId clientId:(NSString *)clientId resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if (peerId.length > 4096 || clientId.length > 64) { reject(@"RUST_INVALID", @"Invalid client identity.", nil); return; }
  [self submit:^id(SfmStatus *status) {
    NSString *identifier = nil;
    *status = [[SFClientStore shared] createPeer:peerId clientId:clientId result:&identifier];
    if (identifier) [self->_lifetime.clients addObject:identifier];
    return identifier;
  } resolve:resolve reject:reject];
}
- (void)afterPoll:(NSString *)identifier admission:(SFClientAdmission)admission work:(SFClientDeferredWork)work
          resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    SFClientRecord *original = [self ownedClient:identifier];
    if (!original) { *status = SFM_STALE; return nil; }
    [[SFClientStore shared] whenDrained:identifier admission:admission work:^id(SfmStatus *actual) {
      if (self->_lifetime->invalidated.load() || [self ownedClient:identifier] != original) { *actual = SFM_STALE; return nil; }
      return work(actual);
    } done:^(id value, SfmStatus actual) {
      if (actual == SFM_OK) resolve(value ?: NSNull.null);
      else reject([NSString stringWithFormat:@"RUST_%d", actual], @"Native operation failed.", nil);
    }];
    return nil;
  } resolve:^(id ignored) {} reject:reject];
}
- (void)pollProfile:(NSString *)identifier profile:(uint32_t)profile
            resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client || client.profile != profile) { *status = SFM_STALE; return nil; }
    uint64_t operation = 0; *status = [[SFClientStore shared] poll:identifier operation:&operation];
    if (*status == SFM_OK) [client.explicitOperations addObject:SFDecimal(operation)];
    return SFDecimal(operation);
  } resolve:resolve reject:reject];
}
- (void)start:(NSString *)identifier function:(SfmStatus (*)(SfmClientHandle, uint64_t *))function
      resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  SFClientAdmission admission = function == sfm_client_reconnect ? SFClientAdmissionV1Reconnect : SFClientAdmissionOrdinary;
  [self afterPoll:identifier admission:admission work:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client) { *status = SFM_STALE; return nil; }
    if (client.profile != SFM_CLIENT_PROFILE_V1) { *status = SFM_INVALID; return nil; }
    uint64_t operation = 0; *status = function(client.handle, &operation);
    if (*status == SFM_OK) [client.explicitOperations addObject:SFDecimal(operation)];
    return SFDecimal(operation);
  } resolve:resolve reject:reject];
}
- (void)connect:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self start:identifier function:sfm_client_connect resolve:resolve reject:reject];
}
- (void)reconnect:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self start:identifier function:sfm_client_reconnect resolve:resolve reject:reject];
}
- (void)poll:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pollProfile:identifier profile:SFM_CLIENT_PROFILE_V1 resolve:resolve reject:reject];
}
- (void)operation:(NSString *)identifier json:(NSString *)json resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [json dataUsingEncoding:NSUTF8StringEncoding];
  if (!bytes || bytes.length > SFM_CLIENT_MAX_REQUEST_BYTES) { reject(@"RUST_INVALID", @"Request is too large.", nil); return; }
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client) { *status = SFM_STALE; return nil; }
    if (client.profile != SFM_CLIENT_PROFILE_V1) { *status = SFM_INVALID; return nil; }
    id command = [NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];
    id methodValue = [command isKindOfClass:NSDictionary.class] ? command[@"method"] : nil;
    NSString *method = [methodValue isKindOfClass:NSString.class] ? methodValue : nil;
    if ([method isEqual:@"writeTerminal"] || [method isEqual:@"acknowledgeTerminal"]) { *status = SFM_INVALID; return nil; }
    if (([method isEqual:@"resizeTerminal"] || [method isEqual:@"finishTerminalUpload"]) &&
      [[SFClientStore shared] hasPendingIntents:identifier]) { *status = SFM_BUSY; return nil; }
    uint64_t operation = 0;
    *status = sfm_client_operation(client.handle, (const uint8_t *)bytes.bytes, bytes.length, &operation);
    if (*status == SFM_OK) [client.explicitOperations addObject:SFDecimal(operation)];
    return SFDecimal(operation);
  } resolve:resolve reject:reject];
}
- (void)cancel:(NSString *)identifier operationId:(NSString *)operationId resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t operation = 0;
  if (!SFParseId(operationId, &operation)) { reject(@"RUST_INVALID", @"Invalid operation ID.", nil); return; }
  [self submit:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    *status = client ? sfm_client_cancel(client.handle, operation) : SFM_STALE;
    return nil;
  } resolve:resolve reject:reject];
}
- (void)bindTerminal:(NSString *)identifier tabId:(NSString *)tabId terminalHandle:(NSString *)terminalId
             resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if (tabId.length > 64) { reject(@"RUST_INVALID", @"Invalid tab ID.", nil); return; }
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    if (![self ownedClient:identifier] || ![self ownsTerminal:terminalId]) { *status = SFM_STALE; return nil; }
    *status = [[SFClientStore shared] bind:identifier tab:tabId terminal:terminalId];
    return nil;
  } resolve:resolve reject:reject];
}
- (void)unbindTerminal:(NSString *)identifier terminalHandle:(NSString *)terminalId
               resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self afterPoll:identifier admission:SFClientAdmissionV1RetireView work:^id(SfmStatus *status) {
    if (![self ownedClient:identifier] || ![self ownsTerminal:terminalId]) { *status = SFM_STALE; return nil; }
    *status = [[SFClientStore shared] unbind:identifier terminal:terminalId];
    return nil;
  } resolve:resolve reject:reject];
}
- (void)readEvent:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    if (![self ownedClient:identifier]) { *status = SFM_STALE; return nil; }
    NSData *data = [[SFClientStore shared] read:identifier];
    return data ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] : nil;
  } resolve:resolve reject:reject];
}
- (void)closeClient:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    uint64_t parsed = 0;
    if (!SFParseId(identifier, &parsed) || ![self->_lifetime.clients containsObject:identifier]) { *status = SFM_STALE; return nil; }
    *status = [[SFClientStore shared] beginClose:identifier completion:^(SfmStatus closed) {
      if (closed == SFM_OK) {
        [self->_lifetime.clients removeObject:identifier]; resolve(NSNull.null);
      } else reject(@"RUST_CLOSE", @"Native client close failed.", nil);
    }];
    return nil;
  } resolve:^(id ignored) {} reject:reject];
}
- (NSNumber *)observerV2AbiVersion { return @(sfm_observer_v2_abi_version()); }
- (void)clientProfile:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self submit:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client) { *status = SFM_STALE; return nil; }
    uint32_t profile = 0, capabilities = 0;
    *status = sfm_client_profile(client.handle, &profile, &capabilities);
    if (*status != SFM_OK) return nil;
    NSData *json = [NSJSONSerialization dataWithJSONObject:@{@"profile":@(profile), @"apiCapabilities":@(capabilities)} options:0 error:nil];
    return [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
  } resolve:resolve reject:reject];
}
- (void)createObserverV2Client:(NSString *)peerId clientId:(NSString *)clientId resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if (peerId.length > 4096 || clientId.length > 64) { reject(@"RUST_INVALID", @"Invalid client identity.", nil); return; }
  [self submit:^id(SfmStatus *status) {
    NSString *identifier = nil;
    *status = [[SFClientStore shared] createObserverPeer:peerId clientId:clientId result:&identifier];
    if (identifier) [self->_lifetime.clients addObject:identifier];
    return identifier;
  } resolve:resolve reject:reject];
}
- (void)observerOperation:(NSString *)identifier action:(SfmStatus (^)(SfmClientHandle, uint64_t *))action
                  resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client) { *status = SFM_STALE; return nil; }
    if (client.profile != SFM_CLIENT_PROFILE_OBSERVER_V2) { *status = SFM_INVALID; return nil; }
    uint64_t operation = 0; *status = action(client.handle, &operation);
    if (*status == SFM_OK) [client.explicitOperations addObject:SFDecimal(operation)];
    return SFDecimal(operation);
  } resolve:resolve reject:reject];
}
- (void)discoverObserverV2:(NSString *)identifier offerLegacy:(BOOL)offer resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self observerOperation:identifier action:^SfmStatus(SfmClientHandle client, uint64_t *operation) {
    return sfm_observer_v2_discover(client, offer ? 1 : 0, operation);
  } resolve:resolve reject:reject];
}
- (void)pairObserverV2:(NSString *)identifier attemptId:(NSString *)attempt resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [attempt dataUsingEncoding:NSUTF8StringEncoding];
  if (bytes.length != 36) { reject(@"RUST_INVALID", @"Invalid attempt ID.", nil); return; }
  [self observerOperation:identifier action:^SfmStatus(SfmClientHandle client, uint64_t *operation) {
    return sfm_observer_v2_pair(client, (const uint8_t *)bytes.bytes, bytes.length, operation);
  } resolve:resolve reject:reject];
}
- (void)observerV2Operation:(NSString *)identifier json:(NSString *)json resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [json dataUsingEncoding:NSUTF8StringEncoding];
  if (!bytes || bytes.length > SFM_CLIENT_MAX_REQUEST_BYTES) { reject(@"RUST_INVALID", @"Request is too large.", nil); return; }
  [self observerOperation:identifier action:^SfmStatus(SfmClientHandle client, uint64_t *operation) {
    return sfm_observer_v2_operation(client, (const uint8_t *)bytes.bytes, bytes.length, operation);
  } resolve:resolve reject:reject];
}
- (void)pollObserverV2:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pollProfile:identifier profile:SFM_CLIENT_PROFILE_OBSERVER_V2 resolve:resolve reject:reject];
}
- (void)reconnectObserverV2:(NSString *)identifier clientId:(NSString *)clientId resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes = [clientId dataUsingEncoding:NSUTF8StringEncoding];
  if (bytes.length != 36) { reject(@"RUST_INVALID", @"Invalid client ID.", nil); return; }
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    SFClientRecord *client = [self ownedClient:identifier];
    if (!client) { *status = SFM_STALE; return nil; }
    if (client.profile != SFM_CLIENT_PROFILE_OBSERVER_V2) { *status = SFM_INVALID; return nil; }
    *status = sfm_observer_v2_reconnect(client.handle, (const uint8_t *)bytes.bytes, bytes.length);
    if (*status == SFM_OK) { client.polling = NO; client.authorityEpoch++; }
    return nil;
  } resolve:resolve reject:reject];
}
- (void)disconnectObserverV2:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self observerOperation:identifier action:^SfmStatus(SfmClientHandle client, uint64_t *operation) {
    return sfm_observer_v2_disconnect(client, operation);
  } resolve:resolve reject:reject];
}
#include "ShellfoxRustManagerMethods.inc"

@end
