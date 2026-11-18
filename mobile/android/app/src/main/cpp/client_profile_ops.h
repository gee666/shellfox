#pragma once
#include "shellfox_mobile_client.h"
namespace client_ops {
struct Api {
  decltype(&sfm_client_bind_terminal) bind;
  decltype(&sfm_client_unbind_terminal) unbind;
  decltype(&sfm_client_terminal_consumed) consumed;
  decltype(&sfm_client_terminal_recovery_ready) ready;
  decltype(&sfm_client_terminal_reset_ticket) ticket;
  decltype(&sfm_client_terminal_reset_grid) grid;
  decltype(&sfm_client_terminal_reset_done) reset;
  decltype(&sfm_client_poll) poll;
  decltype(&sfm_client_disconnect) disconnect;
  decltype(&sfm_client_operation) operation;
};
inline const Api* forProfile(uint32_t profile) {
  static const Api legacy{sfm_client_bind_terminal,sfm_client_unbind_terminal,sfm_client_terminal_consumed,sfm_client_terminal_recovery_ready,sfm_client_terminal_reset_ticket,sfm_client_terminal_reset_grid,sfm_client_terminal_reset_done,sfm_client_poll,sfm_client_disconnect,sfm_client_operation};
  static const Api observer{sfm_observer_v2_bind_terminal,sfm_observer_v2_unbind_terminal,sfm_observer_v2_terminal_consumed,sfm_observer_v2_terminal_recovery_ready,sfm_observer_v2_terminal_reset_ticket,sfm_observer_v2_terminal_reset_grid,sfm_observer_v2_terminal_reset_done,sfm_observer_v2_poll,sfm_observer_v2_disconnect,sfm_observer_v2_operation};
  if(profile==SFM_CLIENT_PROFILE_V1)return &legacy;
  if(profile==SFM_CLIENT_PROFILE_OBSERVER_V2)return &observer;
#ifdef SFM_ENABLE_MANAGER_V2
  // Native0 installs fresh on the original engine owner. No legacy reset API.
  static const Api manager{sfm_manager_v2_bind_terminal,sfm_manager_v2_unbind_terminal,sfm_manager_v2_terminal_consumed,sfm_manager_v2_terminal_recovery_ready,nullptr,nullptr,nullptr,sfm_manager_v2_poll,sfm_manager_v2_disconnect,sfm_manager_v2_operation};
  if(profile==SFM_CLIENT_PROFILE_MANAGER_V2)return &manager;
#endif
  return nullptr;
}
inline SfmStatus resize(uint32_t profile,SfmClientHandle client,SfmHandle engine,uint16_t cols,uint16_t rows,uint64_t* op){
#ifdef SFM_ENABLE_MANAGER_V2
  if(profile==SFM_CLIENT_PROFILE_MANAGER_V2)return sfm_manager_v2_terminal_resize(client,engine,cols,rows,op);
#endif
  return profile==SFM_CLIENT_PROFILE_V1?sfm_client_terminal_resize(client,engine,cols,rows,op):SFM_INVALID;
}
inline bool statusTransport(uint32_t profile){return profile==SFM_CLIENT_PROFILE_OBSERVER_V2||profile==SFM_CLIENT_PROFILE_MANAGER_V2;}
inline SfmStatus completeHttp(uint32_t profile,SfmClientHandle client,uint64_t generation,uint64_t request,int32_t outcome,uint16_t status,const uint8_t* bytes,size_t length){
#ifdef SFM_ENABLE_MANAGER_V2
  if(profile==SFM_CLIENT_PROFILE_MANAGER_V2)return sfm_manager_v2_complete_http(client,generation,request,outcome,status,bytes,length);
#endif
  if(profile==SFM_CLIENT_PROFILE_OBSERVER_V2)return sfm_observer_v2_complete_http(client,generation,request,outcome,status,bytes,length);
  return SFM_INVALID;
}
}
