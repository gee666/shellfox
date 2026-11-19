// Included inside the existing native module implementation, not a new module.
- (NSNumber *)managerV2AbiVersion {
#ifdef SFM_ENABLE_MANAGER_V2
  return @(sfm_manager_v2_abi_version());
#else
  return @0;
#endif
}
- (void)createManagerV2Client:(NSString *)peer clientId:(NSString *)identity resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  if(peer.length>4096||identity.length>36){reject(@"RUST_INVALID",@"Invalid manager identity.",nil);return;}
  [self submit:^id(SfmStatus *status){NSString *identifier=nil;*status=[[SFClientStore shared] createManagerPeer:peer clientId:identity result:&identifier];if(identifier)[self->_lifetime.clients addObject:identifier];return identifier;} resolve:resolve reject:reject];
}
- (void)managerOperation:(NSString *)identifier action:(SfmStatus (^)(SfmClientHandle,uint64_t *))action resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status){SFClientRecord *client=[self ownedClient:identifier];if(!client){*status=SFM_STALE;return nil;}if(client.profile!=SFM_CLIENT_PROFILE_MANAGER_V2){*status=SFM_INVALID;return nil;}uint64_t op=0;*status=action(client.handle,&op);if(*status==SFM_OK)[client.explicitOperations addObject:SFDecimal(op)];return SFDecimal(op);} resolve:resolve reject:reject];
}
- (void)discoverManagerV2:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self managerOperation:identifier action:^SfmStatus(SfmClientHandle client,uint64_t *op){
#ifdef SFM_ENABLE_MANAGER_V2
    return sfm_manager_v2_discover(client,op);
#else
    (void)client;(void)op;return SFM_CLIENT_PROTOCOL;
#endif
  } resolve:resolve reject:reject];
}
- (void)pairManagerV2:(NSString *)identifier attemptId:(NSString *)attempt resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes=[attempt dataUsingEncoding:NSUTF8StringEncoding];if(bytes.length!=36){reject(@"RUST_INVALID",@"Invalid attempt ID.",nil);return;}
  [self managerOperation:identifier action:^SfmStatus(SfmClientHandle client,uint64_t *op){
#ifdef SFM_ENABLE_MANAGER_V2
    return sfm_manager_v2_pair(client,(const uint8_t *)bytes.bytes,bytes.length,op);
#else
    (void)client;(void)op;return SFM_CLIENT_PROTOCOL;
#endif
  } resolve:resolve reject:reject];
}
- (void)managerV2Operation:(NSString *)identifier json:(NSString *)json resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes=[json dataUsingEncoding:NSUTF8StringEncoding];if(!bytes||bytes.length>SFM_CLIENT_MAX_REQUEST_BYTES){reject(@"RUST_INVALID",@"Manager request too large.",nil);return;}
  [self managerOperation:identifier action:^SfmStatus(SfmClientHandle client,uint64_t *op){
#ifdef SFM_ENABLE_MANAGER_V2
    return sfm_manager_v2_operation(client,(const uint8_t *)bytes.bytes,bytes.length,op);
#else
    (void)client;(void)op;return SFM_CLIENT_PROTOCOL;
#endif
  } resolve:resolve reject:reject];
}
- (void)pollManagerV2:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pollProfile:identifier profile:SFM_CLIENT_PROFILE_MANAGER_V2 resolve:resolve reject:reject];
}
- (void)disconnectManagerV2:(NSString *)identifier resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self managerOperation:identifier action:^SfmStatus(SfmClientHandle client,uint64_t *op){
#ifdef SFM_ENABLE_MANAGER_V2
    return sfm_manager_v2_disconnect(client,op);
#else
    (void)client;(void)op;return SFM_CLIENT_PROTOCOL;
#endif
  } resolve:resolve reject:reject];
}
- (void)reconnectManagerV2:(NSString *)identifier clientId:(NSString *)identity resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  NSData *bytes=[identity dataUsingEncoding:NSUTF8StringEncoding];if(bytes.length!=36){reject(@"RUST_INVALID",@"Invalid client ID.",nil);return;}
  [self afterPoll:identifier admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status){SFClientRecord *client=[self ownedClient:identifier];if(!client){*status=SFM_STALE;return nil;}if(client.profile!=SFM_CLIENT_PROFILE_MANAGER_V2){*status=SFM_INVALID;return nil;}
#ifdef SFM_ENABLE_MANAGER_V2
    *status=sfm_manager_v2_reconnect(client.handle,(const uint8_t *)bytes.bytes,bytes.length);if(*status==SFM_OK){client.polling=NO;client.authorityEpoch++;}
#else
    *status=SFM_CLIENT_PROTOCOL;
#endif
    return nil;} resolve:resolve reject:reject];
}
