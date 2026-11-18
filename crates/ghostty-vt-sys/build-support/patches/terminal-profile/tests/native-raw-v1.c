/* Compare this exact Native raw-v1 fixture across checkpoint2 and layout2 builds. */
#include <ghostty/vt.h>
#include <stdio.h>
#include <string.h>
int main(int argc,char**argv) {
  if(argc!=2) return 1;
  GhosttyTerminal t=NULL;if(ghostty_terminal_new(NULL,&t,8,3)!=GHOSTTY_SUCCESS) return 2;
  const size_t cap=4096;
  if(ghostty_terminal_set(t,GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,&cap)!=GHOSTTY_SUCCESS) return 3;
  const char bytes[]="A\xcc\x81\xe4\xb8\xad\xf0\x9f\x98\x80\x1b[48;2;90;100;110m \x1b[0m\x1b]8;;https://example.invalid\x1b\\Z\x1b]8;;\x1b\\\x1b[?1049hALT\x1b[?1049l\x1b[31";
  ghostty_terminal_vt_write(t,(const uint8_t*)bytes,strlen(bytes));
  uint8_t snapshot[65536];size_t length=0;
  if(ghostty_snapshot_encode_buf(t,snapshot,sizeof(snapshot),&length)!=GHOSTTY_SUCCESS) return 4;
  FILE*out=fopen(argv[1],"wb");if(!out) return 5;
  if(fwrite(snapshot,1,length,out)!=length) return 6;
  fclose(out);ghostty_terminal_free(t);return 0;
}
