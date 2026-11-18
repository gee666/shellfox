#import "SFClientProfile.h"

namespace shellfox {
const ClientAPI &V1ClientAPI() {
  static const ClientAPI api{SFM_CLIENT_PROFILE_V1,
    SFM_CLIENT_API_DELIVERY | SFM_CLIENT_API_INPUT | SFM_CLIENT_API_HOST_LAYOUT,
    sfm_client_bind_terminal, sfm_client_unbind_terminal,
    sfm_client_terminal_reset_ticket, sfm_client_terminal_reset_grid,
    sfm_client_terminal_reset_done, sfm_client_terminal_recovery_ready,
    sfm_client_terminal_consumed, sfm_client_poll, sfm_client_disconnect};
  return api;
}
const ClientAPI &ObserverClientAPI() {
  static const ClientAPI api{SFM_CLIENT_PROFILE_OBSERVER_V2, SFM_CLIENT_API_DELIVERY,
    sfm_observer_v2_bind_terminal, sfm_observer_v2_unbind_terminal,
    sfm_observer_v2_terminal_reset_ticket, sfm_observer_v2_terminal_reset_grid,
    sfm_observer_v2_terminal_reset_done, sfm_observer_v2_terminal_recovery_ready,
    sfm_observer_v2_terminal_consumed, sfm_observer_v2_poll, sfm_observer_v2_disconnect};
  return api;
}
const ClientAPI *ClientAPIForProfile(uint32_t profile) {
  if(profile==SFM_CLIENT_PROFILE_V1)return &V1ClientAPI();
  if(profile==SFM_CLIENT_PROFILE_OBSERVER_V2)return &ObserverClientAPI();
#ifdef SFM_ENABLE_MANAGER_V2
  static const ClientAPI manager{SFM_CLIENT_PROFILE_MANAGER_V2,SFM_CLIENT_API_DELIVERY|SFM_CLIENT_API_INPUT|SFM_CLIENT_API_HOST_LAYOUT,
    sfm_manager_v2_bind_terminal,sfm_manager_v2_unbind_terminal,nullptr,nullptr,nullptr,
    sfm_manager_v2_terminal_recovery_ready,sfm_manager_v2_terminal_consumed,sfm_manager_v2_poll,sfm_manager_v2_disconnect};
  if(profile==SFM_CLIENT_PROFILE_MANAGER_V2)return &manager;
#endif
  return nullptr;
}
}
