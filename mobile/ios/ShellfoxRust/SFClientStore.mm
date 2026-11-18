#import "SFClientStore.h"
#include <cstdlib>


@implementation SFClientRecord
- (instancetype)init {
  if ((self = [super init])) {
    _events = [NSMutableArray new]; _resets = [NSMutableSet new];
    _explicitOperations = [NSMutableSet new]; _resetTickets = [NSMutableDictionary new];
    _closeWaiters = [NSMutableArray new];
    _pendingCalls = [NSMutableArray new]; _mutations = [NSMutableSet new];
    _profile = SFM_CLIENT_PROFILE_V1;
    _apiCapabilities = shellfox::V1ClientAPI().capabilities;
  }
  return self;
}
- (const shellfox::ClientAPI &)functions {
  const auto *api=shellfox::ClientAPIForProfile(self.profile);
  if(!api)@throw [NSException exceptionWithName:NSInvalidArgumentException reason:@"Unsupported native client profile" userInfo:nil];
  return *api;
}
@end

@implementation SFClientStore {
  NSMutableDictionary<NSString *, SFClientRecord *> *_clients;
  NSMutableDictionary<NSString *, NSString *> *_bindings;
  std::atomic<bool> _foreground;
  NSArray *_observers;
}
+ (instancetype)shared {
  static SFClientStore *store;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ store = [SFClientStore new]; });
  return store;
}
- (instancetype)init {
  if ((self = [super init])) {
    _clients = [NSMutableDictionary new]; _bindings = [NSMutableDictionary new];
    _foreground.store(true);
    __weak SFClientStore *weakSelf = self;
    _observers = @[
      [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidEnterBackgroundNotification object:nil queue:nil usingBlock:^(NSNotification *note) {
        SFClientStore *store = weakSelf; if (store) store->_foreground.store(false);
      }],
      [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationWillEnterForegroundNotification object:nil queue:nil usingBlock:^(NSNotification *note) {
        SFClientStore *store = weakSelf; if (store) store->_foreground.store(true);
      }]];
  }
  return self;
}
- (SFClientRecord *)client:(NSString *)identifier { return _clients[identifier]; }
- (BOOL)isBound:(NSString *)terminal client:(SFClientRecord *)client {
  return [_bindings[terminal] isEqual:SFDecimal(client.handle)];
}
- (BOOL)recoveringViews:(SFClientRecord *)client {
  for (NSString *identifier in _bindings) if ([self isBound:identifier client:client]) {
    SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
    if (!terminal || terminal.recovering || [client.resets containsObject:identifier]) return YES;
  }
  return NO;
}
- (BOOL)ownerDrained:(SFClientRecord *)client {
  for (NSString *identifier in _bindings) if ([self isBound:identifier client:client]) {
    SFTerminal *terminal = [[SFOwner shared] terminal:identifier]; SfmProgress progress{};
    if (!terminal || sfm_terminal_progress(terminal.handle, &progress) != SFM_OK ||
      progress.accepted_sequence != progress.consumed_sequence || progress.queued_bytes || progress.queued_chunks) return NO;
  }
  return YES;
}
- (SfmStatus)createPeer:(NSString *)peer clientId:(NSString *)identity result:(NSString **)result {
  return [self createPeer:peer clientId:identity profile:SFM_CLIENT_PROFILE_V1 result:result];
}
- (SfmStatus)createObserverPeer:(NSString *)peer clientId:(NSString *)identity result:(NSString **)result {
  return [self createPeer:peer clientId:identity profile:SFM_CLIENT_PROFILE_OBSERVER_V2 result:result];
}
- (SfmStatus)createManagerPeer:(NSString *)peer clientId:(NSString *)identity result:(NSString **)result {
  return [self createPeer:peer clientId:identity profile:SFM_CLIENT_PROFILE_MANAGER_V2 result:result];
}
- (SfmStatus)createPeer:(NSString *)peer clientId:(NSString *)identity profile:(uint32_t)requested result:(NSString **)result {
  const auto *expected=shellfox::ClientAPIForProfile(requested);
  if(!expected||sfm_client_abi_version()!=SFM_CLIENT_ABI_VERSION)return SFM_INVALID;
  auto create=sfm_client_create;
  if(requested==SFM_CLIENT_PROFILE_OBSERVER_V2){if(sfm_observer_v2_abi_version()!=1)return SFM_INVALID;create=sfm_observer_v2_create;}
#ifdef SFM_ENABLE_MANAGER_V2
  if(requested==SFM_CLIENT_PROFILE_MANAGER_V2){if(sfm_manager_v2_abi_version()!=1)return SFM_INVALID;create=sfm_manager_v2_create;}
#endif
  NSData *p=[peer dataUsingEncoding:NSUTF8StringEncoding],*c=[identity dataUsingEncoding:NSUTF8StringEncoding];
  SfmClientHandle handle=0;SfmStatus status=create((const uint8_t *)p.bytes,p.length,(const uint8_t *)c.bytes,c.length,&handle);
  if(status!=SFM_OK)return status;
  uint32_t profile=0,capabilities=0;status=sfm_client_profile(handle,&profile,&capabilities);
  if(status!=SFM_OK||profile!=expected->profile||capabilities!=expected->capabilities){[[SFGoTransport shared] closeClient:handle];return SFM_INVALID;}
  NSString *identifier=SFDecimal(handle);SFClientRecord *record=[SFClientRecord new];record.handle=handle;record.profile=profile;record.apiCapabilities=capabilities;
  _clients[identifier]=record;*result=identifier;return SFM_OK;
}
- (void)dropBindings:(NSString *)identifier {
  SFClientRecord *client = _clients[identifier];
  for (NSString *terminalId in _bindings.allKeys) if ([_bindings[terminalId] isEqual:identifier]) {
    SFTerminal *terminal = [[SFOwner shared] terminal:terminalId];
    if (terminal) {
      // The SDK client has actually closed and retired its own bindings.
      terminal.sink = nil; terminal.recovering = NO; terminal.recoveryInstalled = NO;
      terminal.mailbox->ready.store(false);
      [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
    }
    [_bindings removeObjectForKey:terminalId];
  }
}
- (SfmStatus)close:(NSString *)identifier { return [self beginClose:identifier completion:nil]; }
- (SfmStatus)beginClose:(NSString *)identifier completion:(SFClientCloseCompletion)completion {
  SFClientRecord *client = _clients[identifier];
  if (!client) return SFM_STALE;
  if (client.closing && (client.disconnectFinished || client.nativeCloseAttempted) && client.closeStatus != SFM_OK) return client.closeStatus;
  if (completion && client.closeWaiters.count >= 32) return SFM_BUSY;
  if (completion) [client.closeWaiters addObject:[completion copy]];
  if (!client.closing) {
    client.closing = YES; client.polling = NO;
    client.closeDeadline = NSProcessInfo.processInfo.systemUptime + 2;
    [self retirePending:client];
    [self startDisconnect:client];
  }
  return SFM_OK;
}
- (void)startDisconnect:(SFClientRecord *)client {
  if (client.disconnectOperation || client.pollOperation || client.mutations.count || ![self ownerDrained:client]) return;
  uint64_t operation = 0;
  SfmStatus status = [client functions].disconnect(client.handle, &operation);
  if (status == SFM_OK) client.disconnectOperation = SFDecimal(operation);
  else if (status != SFM_BUSY) { client.disconnectFinished = YES; client.closeStatus = status; }
}
- (void)finishClose:(SFClientRecord *)client {
  if (client.nativeCloseAttempted) return;
  client.nativeCloseAttempted = YES;
  SfmStatus status = [[SFGoTransport shared] closeClient:client.handle];
  if (status == SFM_OK) {
    [self dropBindings:SFDecimal(client.handle)];
    [_clients removeObjectForKey:SFDecimal(client.handle)];
  } else client.closeStatus = status;
  NSArray<SFClientCloseCompletion> *waiters = client.closeWaiters.copy;
  [client.closeWaiters removeAllObjects];
  for (SFClientCloseCompletion waiter in waiters) waiter(status);
}
- (SfmStatus)bind:(NSString *)identifier tab:(NSString *)tab terminal:(NSString *)terminalId {
  SFClientRecord *client = _clients[identifier];
  SFTerminal *terminal = [[SFOwner shared] terminal:terminalId];
  if (!client || client.closing || !terminal) return SFM_STALE;
  NSData *bytes = [tab dataUsingEncoding:NSUTF8StringEncoding];
  SfmStatus status = [client functions].bind(client.handle, (const uint8_t *)bytes.bytes, bytes.length, terminal.handle);
  if (status == SFM_OK) {
    _bindings[terminalId] = identifier; terminal.sink = self;
    terminal.deliveryOnly = client.profile == SFM_CLIENT_PROFILE_OBSERVER_V2;
    terminal.hostEncoding = client.profile == SFM_CLIENT_PROFILE_MANAGER_V2;
    terminal.mailbox->deliveryOnly.store(terminal.deliveryOnly);
    terminal.mailbox->hostEncoding.store(terminal.hostEncoding);
    [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
    terminal.recovering = YES; terminal.recoveryInstalled = terminal.hostEncoding;
    terminal.mailbox->ready.store(false);
  }
  return status;
}
- (SfmStatus)unbind:(NSString *)identifier terminal:(NSString *)terminalId {
  SFClientRecord *client = _clients[identifier];
  SFTerminal *terminal = [[SFOwner shared] terminal:terminalId];
  if (!client || !terminal || ![_bindings[terminalId] isEqual:identifier]) return SFM_STALE;
  SfmStatus status = [client functions].unbind(client.handle, terminal.handle);
  if (status == SFM_OK || status == SFM_STALE) {
    terminal.sink = nil; terminal.recovering = NO; terminal.recoveryInstalled = NO;
    terminal.mailbox->ready.store(false);
    [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
    [_bindings removeObjectForKey:terminalId];
    [client.resets removeObject:terminalId]; [client.resetTickets removeObjectForKey:terminalId];
  }
  return status;
}
- (void)tick {
  for (SFClientRecord *client in _clients.allValues) {
    if (client.closing && NSProcessInfo.processInfo.systemUptime >= client.closeDeadline && !client.disconnectFinished) {
      NSArray<SFClientCloseCompletion> *waiters = client.closeWaiters.copy; [client.closeWaiters removeAllObjects];
      for (SFClientCloseCompletion waiter in waiters) waiter(SFM_BUSY);
    }
    if (client.closing && !client.pollOperation && !client.mutations.count && [self ownerDrained:client] &&
      client.disconnectFinished && client.closeStatus == SFM_OK && !client.nativeCloseAttempted) {
      [self finishClose:client]; continue;
    }
    if (client.closing) [self startDisconnect:client];
    if (_foreground.load() || client.closing) [[SFGoTransport shared] dispatchClient:client.handle];
    // Bounded native coarse queue. Rust retains the next event under pressure.
    for (unsigned i = 0; i < 4 && (client.closing || client.events.count < 64); ++i) {
      size_t needed = 0;
      SfmStatus status = sfm_client_read_event(client.handle, nullptr, 0, &needed);
      if (status == SFM_CLIENT_EMPTY) break;
      constexpr size_t maxEvent = SFM_CLIENT_MAX_RESPONSE_BYTES + 65536;
      constexpr size_t maxQueued = 16 * 1024 * 1024;
      if (status != SFM_CLIENT_BUFFER || !needed || needed > maxEvent ||
        (!client.closing && needed > maxQueued - client.eventBytes)) break;
      NSMutableData *data = [NSMutableData dataWithLength:needed];
      status = sfm_client_read_event(client.handle, (uint8_t *)data.mutableBytes, needed, &needed);
      if (status != SFM_OK) break;
      data.length = needed;
      id event = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
      if ([event isKindOfClass:NSDictionary.class]) [self completed:event client:client];
      if (client.closing) {
        if ([event isKindOfClass:NSDictionary.class] && [client.disconnectOperation isEqual:event[@"operationId"]]) {
          client.disconnectFinished = YES;
          NSDictionary *result = [event[@"result"] isKindOfClass:NSDictionary.class] ? event[@"result"] : @{};
          client.closeStatus = [event[@"status"] intValue] == SFM_OK && [result[@"ok"] isEqual:@YES] ? SFM_OK : SFM_ENGINE;
          if (client.closeStatus != SFM_OK) {
            NSArray<SFClientCloseCompletion> *waiters = client.closeWaiters.copy; [client.closeWaiters removeAllObjects];
            for (SFClientCloseCompletion waiter in waiters) waiter(client.closeStatus);
          }
        }
        continue;
      }
      if ([event isKindOfClass:NSDictionary.class] && [event[@"resetTerminals"] isKindOfClass:NSArray.class]) {
        for (id terminal in event[@"resetTerminals"]) if ([terminal isKindOfClass:NSString.class]) [client.resets addObject:terminal];
      }
      if ([self shouldForward:event client:client]) {
        [client.events addObject:data]; client.eventBytes += data.length;
      }
    }
    if (client.closing) continue;
    [self drainClient:client];
    // Native0 imports atomically on owner pump. It has no ANSI reset API.
    if (![client functions].resetTicket) continue;
    // Query each bound engine too. A full JS event queue must not delay reset.
    for (NSString *identifier in _bindings) if ([_bindings[identifier] isEqual:SFDecimal(client.handle)])
      [client.resets addObject:identifier];
    for (NSString *identifier in client.resets.allObjects) {
      SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
      if (!terminal || ![_bindings[identifier] isEqual:SFDecimal(client.handle)]) {
        [client.resets removeObject:identifier]; continue;
      }
      uint64_t ticket = 0;
      SfmStatus status = [client functions].resetTicket(client.handle, terminal.handle, &ticket);
      if (status == SFM_CLIENT_EMPTY) { [client.resets removeObject:identifier]; continue; }
      if (status == SFM_OK) {
        terminal.recovering = YES; terminal.recoveryInstalled = NO;
        terminal.mailbox->ready.store(false);
        [terminal.pendingIntents clear]; terminal.viewportUnaccepted = NO;
      }
      uint16_t cols = terminal.columns, rows = terminal.rows;
      if (status == SFM_OK) {
        uint16_t hostCols = 0, hostRows = 0;
        status = [client functions].resetGrid(client.handle, terminal.handle, ticket, &hostCols, &hostRows);
        if (status == SFM_OK) { cols = hostCols; rows = hostRows; }
        else if (status == SFM_CLIENT_EMPTY) status = SFM_OK;
      }
      if (status == SFM_OK) status = sfm_terminal_reset(terminal.handle, cols, rows);
      if (status == SFM_OK) {
        terminal.columns = cols; terminal.rows = rows;
        terminal.mailbox->resetFrame(); terminal.delivered = 0;
        status = [client functions].resetDone(client.handle, terminal.handle, ticket);
      }
      if (status == SFM_OK) {
        terminal.recoveryInstalled = YES; terminal.dirty = YES;
        terminal.visible = terminal.mailbox->active.load();
        [client.resets removeObject:identifier];
      } else if (status != SFM_BUSY && status != SFM_STALE) [self terminalFailed:identifier status:status];
    }
  }
  if (_foreground.load()) {
    for (NSString *identifier in _clients) {
      SFClientRecord *client = _clients[identifier];
      if (!client.polling || client.pollOperation || client.nextPoll > NSProcessInfo.processInfo.systemUptime) continue;
      uint64_t operation = 0;
      SfmStatus status = [self poll:identifier operation:&operation];
      if (status != SFM_OK) client.nextPoll = NSProcessInfo.processInfo.systemUptime + 0.1;
    }
  }
}
- (BOOL)hasPendingIntents:(NSString *)identifier {
  for (NSString *terminal in _bindings) if ([_bindings[terminal] isEqual:identifier] &&
    [[SFOwner shared] terminal:terminal].pendingIntents.count) return YES;
  return NO;
}
- (NSData *)read:(NSString *)identifier {
  SFClientRecord *client = _clients[identifier];
  NSData *data = client.events.firstObject;
  if (data) { [client.events removeObjectAtIndex:0]; client.eventBytes -= data.length; }
  return data;
}
- (SfmStatus)sendInput:(NSData *)bytes userInitiated:(uint8_t)hint terminal:(NSString *)identifier {
  SFClientRecord *client = _clients[_bindings[identifier]];
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (!client || client.closing || !terminal) return SFM_STALE;
  if (client.profile != SFM_CLIENT_PROFILE_V1 || terminal.deliveryOnly) return SFM_INVALID;
  uint64_t operation = 0;
  return sfm_client_terminal_write_ex(client.handle, terminal.handle, (const uint8_t *)bytes.bytes, bytes.length, hint, &operation);
}
- (SfmStatus)sendCanonicalInput:(NSData *)json terminal:(NSString *)identifier {
#ifdef SFM_ENABLE_MANAGER_V2
  SFClientRecord *client = _clients[_bindings[identifier]];
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (!client || client.closing || !terminal) return SFM_STALE;
  if (!terminal.hostEncoding || terminal.deliveryOnly) return SFM_INVALID;
  uint64_t operation = 0;
  return sfm_manager_v2_terminal_input(client.handle, terminal.handle, (const uint8_t *)json.bytes, json.length, 1, &operation);
#else
  return SFM_INVALID;
#endif
}
- (SfmStatus)resizeTerminal:(NSString *)identifier columns:(uint16_t)columns rows:(uint16_t)rows {
  SFClientRecord *client = _clients[_bindings[identifier]];
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (!client || client.closing || !terminal || terminal.readOnly) return SFM_STALE;
  if (terminal.deliveryOnly) return SFM_INVALID;
  uint64_t operation = 0;
#ifdef SFM_ENABLE_MANAGER_V2
  if (terminal.hostEncoding) return sfm_manager_v2_terminal_resize(client.handle, terminal.handle, columns, rows, &operation);
#endif
  if (client.profile != SFM_CLIENT_PROFILE_V1) return SFM_INVALID;
  return sfm_client_terminal_resize(client.handle, terminal.handle, columns, rows, &operation);
}
- (SfmStatus)consumedTerminal:(NSString *)identifier sequence:(uint64_t)sequence {
  SFClientRecord *client = _clients[_bindings[identifier]];
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (!client || !terminal) return SFM_STALE;
  return [client functions].consumed(client.handle, terminal.handle);
}
- (SfmStatus)recoveryReady:(NSString *)identifier ready:(uint8_t *)ready {
  SFClientRecord *client = _clients[_bindings[identifier]];
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (!client || client.closing || !terminal) return SFM_STALE;
  return [client functions].ready(client.handle, terminal.handle, ready);
}
- (void)terminalFailed:(NSString *)identifier status:(SfmStatus)status {
  SFTerminal *terminal = [[SFOwner shared] terminal:identifier];
  if (terminal) terminal.mailbox->failure.store(status);
}
@end
