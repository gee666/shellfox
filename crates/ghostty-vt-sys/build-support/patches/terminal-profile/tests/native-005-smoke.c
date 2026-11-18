/* Public-only Native/default smoke for private005. No layout casts or new ABI. */
#include <ghostty/vt.h>
#include <ghostty/vt/shellfox_extension.h>
#include <ghostty/vt/shellfox_terminal_profile.h>
#include <stddef.h>
_Static_assert(sizeof(GhosttyShellfoxTerminalProfileState)==(sizeof(size_t)==8?24:16),"Profile ABI1 unchanged");
int main(void) {
  GhosttyShellfoxExtensionInfo info={0};info.size=sizeof(info);info.abi_version=1;
  if(ghostty_shellfox_extension_info(&info)!=GHOSTTY_SUCCESS || info.terminal_profile_abi_version!=1 || info.snapshot_budget_abi_version!=1) return 1;
  const uint8_t identity[]="8888888888888888888888888888888888888888888888888888888888888888";
  GhosttyShellfoxTerminalProfileOptions options={sizeof(options),1,{identity,64},0,8,3};
  GhosttyTerminal t=NULL;
  if(ghostty_shellfox_terminal_new_profile(NULL,&t,&options)!=GHOSTTY_SUCCESS || !t) return 2;
  GhosttyShellfoxTerminalProfileState state={sizeof(state),1,99,99};
  if(ghostty_shellfox_terminal_get_profile_state(t,&state)!=GHOSTTY_SUCCESS || state.terminal_profile || state.preceding_join) return 3;
  ghostty_terminal_free(t);t=NULL;
  options.terminal_profile=1;
  if(ghostty_shellfox_terminal_new_profile(NULL,&t,&options)!=GHOSTTY_UNSUPPORTED || t) return 4;
  return 0;
}
