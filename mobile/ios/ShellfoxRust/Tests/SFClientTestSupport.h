#pragma once
#import "../SFClientStore.h"

// Fixture-only event loop. Call from one owner block, with an injected transport.
static inline NSDictionary *SFTestClientEvent(SfmClientHandle client, SFGoTransport *transport) {
  NSTimeInterval deadline = NSProcessInfo.processInfo.systemUptime + 3;
  while (NSProcessInfo.processInfo.systemUptime < deadline) {
    [transport dispatchClient:client];
    size_t needed = 0;
    SfmStatus status = sfm_client_read_event(client, nullptr, 0, &needed);
    if (status == SFM_CLIENT_BUFFER && needed && needed <= SFM_CLIENT_MAX_RESPONSE_BYTES + 65536) {
      NSMutableData *bytes = [NSMutableData dataWithLength:needed];
      if (sfm_client_read_event(client, (uint8_t *)bytes.mutableBytes, bytes.length, &needed) != SFM_OK) return nil;
      bytes.length = needed;
      id result = [NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];
      return [result isKindOfClass:NSDictionary.class] ? result : nil;
    }
    if (status != SFM_CLIENT_EMPTY) return nil;
    [NSThread sleepForTimeInterval:0.001];
  }
  return nil;
}
static inline BOOL SFTestClientSuccess(NSDictionary *event) {
  return event && [event[@"status"] intValue] == SFM_OK;
}
