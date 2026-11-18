/* Constructor/default boundary smoke only, not Legacy app availability. */
#include <ghostty/vt.h>
#include <ghostty/vt/shellfox_terminal_profile.h>
#include <ghostty/vt/shellfox_extension.h>
#include <stdio.h>
#include <string.h>
#define CHECK(x) do{if(!(x)){fprintf(stderr,"line%d %s\n",__LINE__,#x);return 1;}}while(0)
int main(int argc,char **argv){
 CHECK(argc==2&&strlen(argv[1])==64);
 GhosttyShellfoxExtensionInfo info={.size=sizeof(info),.abi_version=1};CHECK(ghostty_shellfox_extension_info(&info)==GHOSTTY_SUCCESS);CHECK(info.terminal_profile_abi_version==1&&info.snapshot_budget_abi_version==1);
 GhosttyShellfoxTerminalProfileOptions options={.size=sizeof(options),.abi_version=1,.engine_profile={.ptr=(const uint8_t*)argv[1],.len=64},.terminal_profile=0,.cols=8,.rows=3};
 GhosttyTerminal t=NULL;CHECK(ghostty_shellfox_terminal_new_profile(NULL,&t,&options)==GHOSTTY_SUCCESS&&t);GhosttyShellfoxTerminalProfileState state={.size=sizeof(state),.abi_version=1};CHECK(ghostty_shellfox_terminal_get_profile_state(t,&state)==GHOSTTY_SUCCESS&&state.terminal_profile==0&&state.preceding_join==0);ghostty_terminal_free(t);
 options.terminal_profile=1;t=NULL;CHECK(ghostty_shellfox_terminal_new_profile(NULL,&t,&options)==GHOSTTY_UNSUPPORTED&&t==NULL);
 puts("P4 producer Native/default boundary passed; public Legacy constructor remains OFF");return 0;
}
