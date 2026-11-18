#include <ghostty/vt/shellfox_terminal_profile.h>
#include <ghostty/vt/shellfox_extension.h>
#include <stddef.h>
_Static_assert(offsetof(GhosttyShellfoxTerminalProfileState,size)==0,"State size first");
_Static_assert(offsetof(GhosttyShellfoxTerminalProfileState,abi_version)==sizeof(size_t),"State ABI order");
_Static_assert(offsetof(GhosttyShellfoxTerminalProfileState,terminal_profile)==sizeof(size_t)+4,"State profile order");
_Static_assert(offsetof(GhosttyShellfoxTerminalProfileState,preceding_join)==sizeof(size_t)+8,"State join order");
_Static_assert(sizeof(GhosttyShellfoxTerminalProfileState)==(sizeof(size_t)==8?24:16),"State sized ABI");
static GhosttyResult (*const make_profile)(const GhosttyAllocator*,GhosttyTerminal*,const GhosttyShellfoxTerminalProfileOptions*)=ghostty_shellfox_terminal_new_profile;
static GhosttyResult (*const get_profile)(GhosttyTerminal,uint32_t*)=ghostty_shellfox_terminal_get_profile;
static GhosttyResult (*const get_state)(GhosttyTerminal,GhosttyShellfoxTerminalProfileState*)=ghostty_shellfox_terminal_get_profile_state;
int main(void) {
  GhosttyShellfoxExtensionInfo info={0};info.size=sizeof(info);info.abi_version=1;
  if(ghostty_shellfox_extension_info(&info)!=GHOSTTY_SUCCESS || info.terminal_profile_abi_version!=1 ||
     info.snapshot_budget_abi_version!=0 || !(info.features&GHOSTTY_SHELLFOX_FEATURE_TERMINAL_PROFILE)) return 5;
  const uint8_t identity[]="1111111111111111111111111111111111111111111111111111111111111111";
  GhosttyShellfoxTerminalProfileOptions options={sizeof(options),1,{identity,64},0,8,3};
  GhosttyTerminal terminal=NULL;
  if(make_profile(NULL,&terminal,&options)!=GHOSTTY_SUCCESS || !terminal) return 1;
  uint32_t profile=99;
  if(get_profile(terminal,&profile)!=GHOSTTY_SUCCESS || profile!=0) return 2;
  GhosttyShellfoxTerminalProfileState state={sizeof(state),1,99,99};
  if(get_state(terminal,&state)!=GHOSTTY_SUCCESS || state.terminal_profile!=0 || state.preceding_join!=0) return 3;
  ghostty_terminal_free(terminal);
  options.terminal_profile=1;terminal=NULL;
  /* Native-only prerequisite checkpoint advertises no Legacy success. */
  if(make_profile(NULL,&terminal,&options)!=GHOSTTY_UNSUPPORTED || terminal) return 4;
  return 0;
}
