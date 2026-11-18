/* Public API baseline probe. Never changes the verified upstream library. */
#include <ghostty/vt.h>
#include <stdio.h>
#include <string.h>
int main(void) {
  const char *inputs[] = {"\xcc\x81" "A", "A\x1b[31m\xcc\x81" "B", "A\xef\xb8\x8f" "B"};
  const char *names[] = {"leading-combining", "control-reset-combining", "nonemoji-vs16"};
  for (size_t i=0;i<3;i++) {
    GhosttyTerminal t=NULL; GhosttyFormatter f=NULL;
    if (ghostty_terminal_new(NULL,&t,8,3)!=GHOSTTY_SUCCESS) return 2;
    ghostty_terminal_vt_write(t,(const uint8_t*)inputs[i],strlen(inputs[i]));
    uint16_t x=0,y=0;
    if (ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_X,&x)!=GHOSTTY_SUCCESS ||
        ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_Y,&y)!=GHOSTTY_SUCCESS) return 3;
    if (ghostty_formatter_terminal_new(NULL,&f,t)!=GHOSTTY_SUCCESS) return 4;
    uint8_t bytes[4096];size_t n=0;
    if (ghostty_formatter_format_buf(f,bytes,sizeof(bytes),&n)!=GHOSTTY_SUCCESS) return 5;
    printf("{\"name\":\"%s\",\"cursor\":[%u,%u],\"plainHex\":\"",names[i],x,y);
    for (size_t k=0;k<n;k++) printf("%02x",bytes[k]);
    puts("\"}");ghostty_formatter_free(f);ghostty_terminal_free(t);
  }
  return 0;
}
