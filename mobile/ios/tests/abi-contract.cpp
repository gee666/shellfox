// Header/layout contract only. This does not compile UIKit or prove an iOS build.
#include "../../../crates/shellfox-mobile/include/shellfox_mobile_client.h"
#include <type_traits>

static_assert(sizeof(void *) == 8, "The supported iOS device/simulator ABIs are 64-bit");
static_assert(sizeof(SfmCell) == 32);
static_assert(sizeof(SfmFrame) == 40);
static_assert(sizeof(SfmProgress) == 24);
static_assert(sizeof(SfmSelection) == 16);
static_assert(offsetof(SfmCell, text_length) == 8);
static_assert(offsetof(SfmCell, flags) == 24);
static_assert(offsetof(SfmFrame, cells) == 16);
static_assert(offsetof(SfmFrame, consumed_sequence) == 32);
static_assert(SFM_ABI_VERSION == 1 && SFM_CLIENT_ABI_VERSION == 1);

using Create = SfmStatus (*)(uint16_t, uint16_t, SfmHandle *);
using Reset = SfmStatus (*)(SfmHandle, uint16_t, uint16_t);
using Frame = SfmStatus (*)(SfmHandle, SfmFrameFn, void *);
using Pump = SfmStatus (*)(SfmHandle, SfmErrorFn, void *);
using Input = SfmStatus (*)(SfmHandle, const uint8_t *, size_t, SfmBytesFn, void *);
using InputExCallback = void (*)(void *, const uint8_t *, size_t, uint8_t);
using InputEx = SfmStatus (*)(SfmHandle, const uint8_t *, size_t, SfmInputExFn, void *);
static_assert(std::is_same_v<SfmInputExFn, InputExCallback>);
using Configure = SfmStatus (*)(SfmHandle, const uint8_t *, size_t);
static_assert(std::is_same_v<decltype(&sfm_terminal_create), Create>);
static_assert(std::is_same_v<decltype(&sfm_terminal_reset), Reset>);
static_assert(std::is_same_v<decltype(&sfm_terminal_resize), Reset>);
static_assert(std::is_same_v<decltype(&sfm_terminal_frame), Frame>);
static_assert(std::is_same_v<decltype(&sfm_terminal_pump), Pump>);
static_assert(std::is_same_v<decltype(&sfm_terminal_input), Input>);
static_assert(std::is_same_v<decltype(&sfm_terminal_input_ex), InputEx>);
static_assert(std::is_same_v<decltype(&sfm_terminal_configure), Configure>);

using Dispatch = SfmStatus (*)(SfmClientHandle, SfmClientRequestFn, SfmClientCancelFn, void *);
using Complete = SfmStatus (*)(SfmClientHandle, uint64_t, uint64_t, int32_t, const uint8_t *, size_t);
using Close = SfmStatus (*)(SfmClientHandle, SfmClientCancelFn, void *);
using Disconnect = SfmStatus (*)(SfmClientHandle, uint64_t *);
using ResetTicket = SfmStatus (*)(SfmClientHandle, SfmHandle, uint64_t *);
using ResetDone = SfmStatus (*)(SfmClientHandle, SfmHandle, uint64_t);
using ResetGrid = SfmStatus (*)(SfmClientHandle, SfmHandle, uint64_t, uint16_t *, uint16_t *);
using RecoveryReady = SfmStatus (*)(SfmClientHandle, SfmHandle, uint8_t *);
using Write = SfmStatus (*)(SfmClientHandle, SfmHandle, const uint8_t *, size_t, uint64_t *);
using WriteEx = SfmStatus (*)(SfmClientHandle, SfmHandle, const uint8_t *, size_t, uint8_t, uint64_t *);
using Resize = SfmStatus (*)(SfmClientHandle, SfmHandle, uint16_t, uint16_t, uint64_t *);
static_assert(std::is_same_v<decltype(&sfm_client_dispatch), Dispatch>);
static_assert(std::is_same_v<decltype(&sfm_client_complete), Complete>);
static_assert(std::is_same_v<decltype(&sfm_client_close), Close>);
static_assert(std::is_same_v<decltype(&sfm_client_disconnect), Disconnect>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_reset_ticket), ResetTicket>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_reset_done), ResetDone>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_reset_grid), ResetGrid>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_recovery_ready), RecoveryReady>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_write), Write>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_write_ex), WriteEx>);
static_assert(std::is_same_v<decltype(&sfm_client_terminal_resize), Resize>);
using Request = void (*)(void *, SfmClientHandle, uint64_t, uint64_t,
  const uint8_t *, size_t, const uint8_t *, size_t, const uint8_t *, size_t, uint32_t, size_t);
static_assert(std::is_same_v<SfmClientRequestFn, Request>);

static_assert(SFM_OBSERVER_V2_ABI_VERSION == 1);
static_assert(SFM_CLIENT_PROFILE_V1 == 1 && SFM_CLIENT_PROFILE_OBSERVER_V2 == 2);
static_assert(SFM_CLIENT_API_DELIVERY == 1 && SFM_CLIENT_API_INPUT == 2 && SFM_CLIENT_API_HOST_LAYOUT == 4);
using Profile = SfmStatus (*)(SfmClientHandle, uint32_t *, uint32_t *);
using ObserverCreate = SfmStatus (*)(const uint8_t *, size_t, const uint8_t *, size_t, SfmClientHandle *);
using Discover = SfmStatus (*)(SfmClientHandle, uint8_t, uint64_t *);
using ObserverOperation = SfmStatus (*)(SfmClientHandle, const uint8_t *, size_t, uint64_t *);
using Reconnect = SfmStatus (*)(SfmClientHandle, const uint8_t *, size_t);
using CompleteHTTP = SfmStatus (*)(SfmClientHandle, uint64_t, uint64_t, int32_t, uint16_t, const uint8_t *, size_t);
using Bind = SfmStatus (*)(SfmClientHandle, const uint8_t *, size_t, SfmHandle);
using Unbind = SfmStatus (*)(SfmClientHandle, SfmHandle);
static_assert(std::is_same_v<decltype(&sfm_client_profile), Profile>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_create), ObserverCreate>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_discover), Discover>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_pair), ObserverOperation>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_operation), ObserverOperation>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_poll), Disconnect>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_disconnect), Disconnect>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_reconnect), Reconnect>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_complete_http), CompleteHTTP>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_bind_terminal), Bind>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_unbind_terminal), Unbind>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_terminal_reset_ticket), ResetTicket>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_terminal_reset_grid), ResetGrid>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_terminal_reset_done), ResetDone>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_terminal_recovery_ready), RecoveryReady>);
static_assert(std::is_same_v<decltype(&sfm_observer_v2_terminal_consumed), Unbind>);
