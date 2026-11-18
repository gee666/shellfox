#pragma once
#include "shellfox_mobile_client.h"

namespace shellfox {
// Available APIs only. These constants are never server permission/control grants.
struct ClientAPI {
  uint32_t profile;
  uint32_t capabilities;
  SfmStatus (*bind)(SfmClientHandle, const uint8_t *, size_t, SfmHandle);
  SfmStatus (*unbind)(SfmClientHandle, SfmHandle);
  SfmStatus (*resetTicket)(SfmClientHandle, SfmHandle, uint64_t *);
  SfmStatus (*resetGrid)(SfmClientHandle, SfmHandle, uint64_t, uint16_t *, uint16_t *);
  SfmStatus (*resetDone)(SfmClientHandle, SfmHandle, uint64_t);
  SfmStatus (*ready)(SfmClientHandle, SfmHandle, uint8_t *);
  SfmStatus (*consumed)(SfmClientHandle, SfmHandle);
  SfmStatus (*poll)(SfmClientHandle, uint64_t *);
  SfmStatus (*disconnect)(SfmClientHandle, uint64_t *);
};
const ClientAPI &V1ClientAPI();
const ClientAPI &ObserverClientAPI();
const ClientAPI *ClientAPIForProfile(uint32_t profile);
}
