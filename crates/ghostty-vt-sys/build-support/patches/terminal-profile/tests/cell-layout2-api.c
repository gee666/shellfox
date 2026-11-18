/* Experimental getter smoke only; no Legacy app/restore availability. */
#include <ghostty/vt.h>
#include <ghostty/vt/shellfox_cell_layout2.h>
int main(void) {
  GhosttyTerminal terminal=NULL;if(ghostty_terminal_new(NULL,&terminal,8,3)!=GHOSTTY_SUCCESS) return 1;
  GhosttyCell narrow=(GhosttyCell)'A'<<2;uint8_t width=99;
  if(ghostty_shellfox_terminal_cell_width(terminal,2,narrow,&width)!=GHOSTTY_SUCCESS || width!=1) return 2;
  if(ghostty_shellfox_terminal_cell_width(terminal,1,narrow,&width)!=GHOSTTY_UNSUPPORTED) return 3;
  const GhosttyCell invalid[]={narrow|((GhosttyCell)1<<23),narrow|((GhosttyCell)3<<46),narrow|((GhosttyCell)1<<48),(GhosttyCell)0x110000<<2,(GhosttyCell)0xd800<<2};
  for(size_t i=0;i<sizeof(invalid)/sizeof(invalid[0]);i++) {
    width=99;if(ghostty_shellfox_terminal_cell_width(terminal,2,invalid[i],&width)!=GHOSTTY_INVALID_VALUE || width!=99) return 4;
  }
  ghostty_terminal_free(terminal);return 0;
}
