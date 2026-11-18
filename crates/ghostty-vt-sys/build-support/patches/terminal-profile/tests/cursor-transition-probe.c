/* Baseline public-C observation only. Never repair an invalid P4 cursor. */
#define main codec_boundary_main
#include "../../snapshot-budget/format2/tests/public-api.c"
#undef main
#include <ghostty/vt/shellfox_layout2_data.h>
#include <ghostty/vt/shellfox_cell_layout2.h>
#include <ghostty/vt/grid_ref.h>
#include <inttypes.h>
static void scalar(uint32_t cp){uint8_t b[4];unsigned n=0;if(cp<128)b[n++]=(uint8_t)cp;else if(cp<2048){b[n++]=(uint8_t)(192|(cp>>6));b[n++]=(uint8_t)(128|(cp&63));}else if(cp<65536){b[n++]=(uint8_t)(224|(cp>>12));b[n++]=(uint8_t)(128|((cp>>6)&63));b[n++]=(uint8_t)(128|(cp&63));}else{b[n++]=(uint8_t)(240|(cp>>18));b[n++]=(uint8_t)(128|((cp>>12)&63));b[n++]=(uint8_t)(128|((cp>>6)&63));b[n++]=(uint8_t)(128|(cp&63));}for(unsigned k=0;k<n;k++)printf("%02x",b[k]);}
static int rows(GhosttyTerminal t){uint16_t height=0;CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_ROWS,&height)==0&&height<=8);putchar('[');for(uint16_t y=0;y<height;y++){GhosttyShellfoxLayout2RowData row={.size=sizeof(row),.abi_version=1,.cell_layout_version=2};GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_ACTIVE,.value.coordinate={.x=0,.y=y}};CHECK(ghostty_shellfox_terminal_layout2_row_get(t,p,&row)==0&&row.backing_columns<=16);printf("%s[",y?",":"");for(uint16_t x=0;x<row.backing_columns;x++){p.value.coordinate.x=x;GhosttyGridRef ref={0};GhosttyCell raw;uint32_t cp[8];size_t n=0;uint8_t width=99;CHECK(ghostty_terminal_grid_ref(t,p,&ref)==0&&ghostty_grid_ref_cell(&ref,&raw)==0);CHECK(ghostty_grid_ref_graphemes(&ref,cp,8,&n)==0&&n<=8);int valid=ghostty_shellfox_terminal_cell_width(t,2,raw,&width);printf("%s{\"hex\":\"",x?",":"");for(size_t k=0;k<n;k++)scalar(cp[k]);printf("\",\"width\":%d}",valid?-1:(int)width);}putchar(']');}putchar(']');return 0;}
typedef struct { const uint8_t *text; size_t len; uint16_t cols,rows; } Step;
typedef struct { const Step *steps; size_t count; } Case;
#include "cursor-transition-cases.h"
int main(int argc,char **argv){CHECK(argc==3);FILE *f=fopen(argv[1],"rb");CHECK(f);CHECK(fseek(f,0,SEEK_END)==0);long length=ftell(f);CHECK(length>0&&length<4*1024*1024);rewind(f);uint8_t *blob=malloc((size_t)length);CHECK(blob);CHECK(fread(blob,1,(size_t)length,f)==(size_t)length&&fclose(f)==0);GhosttyShellfoxSnapshotFormat2Options o=opts(argv[2]);
 for(unsigned i=0;i<sizeof(cases)/sizeof(cases[0]);i++){GhosttyTerminal t=NULL;CHECK(ghostty_shellfox_snapshot_format2_decode_buf(blob,(size_t)length,&o,&t,NULL)==0);CHECK(ghostty_terminal_resize(t,4,4,0,0)==0);
 for(unsigned j=0;j<cases[i].count;j++){const Step *step=&cases[i].steps[j];if(step->cols)CHECK(ghostty_terminal_resize(t,step->cols,step->rows,0,0)==0);else ghostty_terminal_vt_write(t,step->text,step->len);
 uint16_t x=0,y=0,height=0;CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_ROWS,&height)==0);bool pending=false;GhosttyTerminalScreen active;CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_X,&x)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_Y,&y)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP,&pending)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,&active)==0);
 GhosttyShellfoxLayout2ScreenData data={.size=sizeof(data),.abi_version=1,.cell_layout_version=2};int status=ghostty_shellfox_terminal_layout2_screen_get(t,&data);GhosttyShellfoxTerminalProfileState state={.size=sizeof(state),.abi_version=1};CHECK(ghostty_shellfox_terminal_get_profile_state(t,&state)==0);
 printf("{\"case\":%u,\"cut\":%u,\"status\":%d,\"physical_x\":%u,\"y\":%u,\"pending\":%u,\"alternate\":%u,\"join\":%u,\"logical_x\":",i,j,status,x,y,(unsigned)pending,(unsigned)active,state.preceding_join);
 if(status)printf("null,\"saved\":null,\"base\":null");else printf("%u,\"saved\":[%u,%u,%u,%u,%u],\"base\":%"PRIu64,(unsigned)x+(unsigned)pending+data.cursor_virtual_overflow,data.saved_present,data.saved_x,data.saved_y,data.saved_pending_wrap,data.saved_virtual_overflow,data.total_rows-height);
 printf(",\"rows\":");CHECK(rows(t)==0);puts("}");}
 ghostty_terminal_free(t);}
 free(blob);return 0;}
