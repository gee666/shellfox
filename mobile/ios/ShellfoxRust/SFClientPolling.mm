#import "SFClientStore.h"
#include <cstdlib>

@interface SFClientDeferredCall : NSObject
@property(nonatomic) NSUInteger epoch;
@property(nonatomic, copy) SFClientDeferredWork work;
@property(nonatomic, copy) SFClientDeferredDone done;
@end
@implementation SFClientDeferredCall
@end

static id RunWork(SFClientRecord *client, SFClientDeferredWork work, SfmStatus *status) {
  id value = work(status);
  uint64_t operation = 0;
  if (*status == SFM_OK && [value isKindOfClass:NSString.class] && SFParseId(value, &operation))
    [client.mutations addObject:value];
  return value;
}

@implementation SFClientStore (Polling)
- (void)whenDrained:(NSString *)identifier admission:(SFClientAdmission)admission
              work:(SFClientDeferredWork)work done:(SFClientDeferredDone)done {
  SFClientRecord *client = [self client:identifier];
  if (!client || client.closing) { done(nil, SFM_STALE); return; }
  if (client.pollFailure != SFM_OK) {
    BOOL teardown = admission == SFClientAdmissionV1RetireView || admission == SFClientAdmissionV1Reconnect;
    if (!teardown) {
      done(nil, client.pollFailure); return;
    }
    // Unbind is local. Reconnect fences the generation in Rust. Neither
    // operation treats an uncertain poll as a successful delivery drain.
    SfmStatus status = SFM_OK;
    id value = RunWork(client, work, &status);
    if (status == SFM_OK && admission == SFClientAdmissionV1Reconnect)
      [self clearFailedPollAfterV1Fence:client];
    done(value, status);
    return;
  }
  // Files commands do not change terminal delivery, and Rust supplies their
  // usual connection/authority checks. Preserve ordering with queued commands.
  if (admission == SFClientAdmissionV1FileRequest &&
      client.polling && !client.pendingCalls.count && !client.mutations.count) {
    SfmStatus status = SFM_OK;
    id value = RunWork(client, work, &status);
    done(value, status);
    return;
  }
  if (client.pendingCalls.count >= 8) { done(nil, SFM_BUSY); return; }
  SFClientDeferredCall *call = [SFClientDeferredCall new];
  call.epoch = client.authorityEpoch; call.work = work; call.done = done;
  [client.pendingCalls addObject:call];
  // Wait for the original normal result. Canceling does not drain delivery.
}
- (void)drainClient:(SFClientRecord *)client {
  while (!client.closing && client.pollFailure == SFM_OK && !client.pollOperation && !client.mutations.count &&
    [self ownerDrained:client] && client.pendingCalls.count) {
    SFClientDeferredCall *call = client.pendingCalls.firstObject;
    [client.pendingCalls removeObjectAtIndex:0];
    if (call.epoch != client.authorityEpoch) { call.done(nil, SFM_STALE); continue; }
    SfmStatus status = SFM_OK;
    id value = RunWork(client, call.work, &status);
    call.done(value, status);
  }
}
- (void)clearFailedPollAfterV1Fence:(SFClientRecord *)client {
  if (client.pollFailure == SFM_OK) return;
  client.pollOperation = nil;
  client.pollFailure = SFM_OK;
  client.polling = NO;
  client.authorityEpoch++;
}
- (void)retirePending:(SFClientRecord *)client {
  client.polling = NO; client.authorityEpoch++;
  NSArray<SFClientDeferredCall *> *pending = client.pendingCalls.copy;
  [client.pendingCalls removeAllObjects];
  for (SFClientDeferredCall *call in pending) call.done(nil, SFM_STALE);
  // Keep the original operation and witness until normal completion and
  // owner processing. A timeout reports BUSY without discarding the slot.
}
- (void)completed:(NSDictionary *)event client:(SFClientRecord *)client {
  NSString *operation = event[@"operationId"];
  if (![operation isKindOfClass:NSString.class]) { client.polling = NO; return; }
  [client.mutations removeObject:operation];
  NSDictionary *result = [event[@"result"] isKindOfClass:NSDictionary.class] ? event[@"result"] : @{};
  BOOL success = [event[@"status"] intValue] == SFM_OK &&
    !([result[@"ok"] isKindOfClass:NSNumber.class] && ![result[@"ok"] boolValue]);
  if ([client.pollOperation isEqual:operation]) {
    if ([event[@"status"] intValue] != SFM_OK) {
      client.pollFailure = [event[@"status"] intValue]; client.polling = NO;
      NSArray<SFClientDeferredCall *> *pending = client.pendingCalls.copy; [client.pendingCalls removeAllObjects];
      for (SFClientDeferredCall *call in pending) call.done(nil, client.pollFailure);
      return;
    }
    client.pollOperation = nil;
    client.nextPoll = NSProcessInfo.processInfo.systemUptime + (success ? 0 : 0.5);
  }
  NSDictionary *connection = [event[@"connection"] isKindOfClass:NSDictionary.class] ? event[@"connection"] : @{};
  id identity = @[connection[@"status"] ?: NSNull.null, connection[@"connectionId"] ?: NSNull.null];
  NSData *bytes = [NSJSONSerialization dataWithJSONObject:identity options:NSJSONWritingSortedKeys error:nil];
  NSString *key = [[NSString alloc] initWithData:bytes encoding:NSUTF8StringEncoding];
  if (![client.authorityKey isEqual:key]) { client.authorityEpoch++; client.authorityKey = key; }
}
- (SfmStatus)poll:(NSString *)identifier operation:(uint64_t *)operation {
  SFClientRecord *client = [self client:identifier];
  if (!client || client.closing) return SFM_STALE;
  if (client.pollFailure != SFM_OK) return client.pollFailure;
  if (client.pollOperation) { *operation = strtoull(client.pollOperation.UTF8String, nullptr, 10); return SFM_OK; }
  if (!client.polling || client.pendingCalls.count || client.mutations.count || [self recoveringViews:client] || ![self ownerDrained:client]) return SFM_BUSY;
  SfmStatus status = sfm_client_poll(client.handle, operation);
  if (status == SFM_OK) client.pollOperation = SFDecimal(*operation);
  return status;
}
- (BOOL)shouldForward:(id)event client:(SFClientRecord *)client {
  if (![event isKindOfClass:NSDictionary.class]) return NO;
  NSString *operation = event[@"operationId"];
  BOOL internal = [event[@"internal"] boolValue];
  BOOL requested = [operation isKindOfClass:NSString.class] && [client.explicitOperations containsObject:operation];
  if (requested) [client.explicitOperations removeObject:operation];
  NSDictionary *connection = [event[@"connection"] isKindOfClass:NSDictionary.class] ? event[@"connection"] : @{};
  NSDictionary *result = [event[@"result"] isKindOfClass:NSDictionary.class] ? event[@"result"] : @{};
  BOOL denied = [result[@"ok"] isKindOfClass:NSNumber.class] && ![result[@"ok"] boolValue];
  NSDictionary *error = [result[@"error"] isKindOfClass:NSDictionary.class] ? result[@"error"] : @{};
  NSString *errorCode = denied ? ([error[@"code"] isKindOfClass:NSString.class] ? error[@"code"] : @"remote") : @"";
  client.polling = client.pollFailure == SFM_OK && [connection[@"status"] isEqual:@"connected"];
  NSString *key = [NSString stringWithFormat:@"%@/%@/%@/%@/%@", connection[@"status"], connection[@"permission"],
      connection[@"connectionId"], event[@"status"], errorCode];
  BOOL changed = ![client.connectionKey isEqual:key]; client.connectionKey = key;
  NSDictionary *tickets = [event[@"resetTickets"] isKindOfClass:NSDictionary.class] ? event[@"resetTickets"] : @{};
  BOOL reset = NO;
  for (NSString *terminal in tickets) if ([self isBound:terminal client:client] &&
    ![client.resetTickets[terminal] isEqual:tickets[terminal]]) {
    client.resetTickets[terminal] = tickets[terminal]; reset = YES;
  }
  NSDictionary *value = [result[@"value"] isKindOfClass:NSDictionary.class] ? result[@"value"] : @{};
  BOOL hostEvents = [value[@"events"] isKindOfClass:NSArray.class] && [value[@"events"] count] > 0;
  return requested || !internal || changed || reset || hostEvents;
}

@end
