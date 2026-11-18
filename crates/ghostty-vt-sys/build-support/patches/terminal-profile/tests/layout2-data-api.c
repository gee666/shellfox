/* Public-only bounded producer/consumer. No packed bits, native layout or second parser. */
#define main existing_codec_boundary_proof_main
#include "../../snapshot-budget/format2/tests/public-api.c"
#undef main
#include <ghostty/vt/shellfox_layout2_data.h>
#include <ghostty/vt/shellfox_cell_layout2.h>
#include <ghostty/vt/grid_ref.h>
#include <inttypes.h>
#define MAX_ROWS 64u
#define MAX_COLS 64u
#define MAX_CP 256u
#define MAX_URI 2048u
#define MAX_BLOB (4u*1024u*1024u)
typedef struct { const uint8_t *text; size_t len; uint16_t resize; } Step;
typedef struct { const char *name; unsigned seed; uint16_t cols; const Step *steps; size_t count; int cursor_x; } Case;
#include "layout2-data-cases.h"
static unsigned kinds_seen;
static int utf8(uint32_t cp,uint8_t *s,size_t *n) {
 if(cp>0x10ffffu||(cp>=0xd800u&&cp<=0xdfffu))return 0;
 if(cp<0x80u)s[(*n)++]=(uint8_t)cp;
 else if(cp<0x800u){s[(*n)++]=(uint8_t)(0xc0u|(cp>>6));s[(*n)++]=(uint8_t)(0x80u|(cp&63u));}
 else if(cp<0x10000u){s[(*n)++]=(uint8_t)(0xe0u|(cp>>12));s[(*n)++]=(uint8_t)(0x80u|((cp>>6)&63u));s[(*n)++]=(uint8_t)(0x80u|(cp&63u));}
 else{s[(*n)++]=(uint8_t)(0xf0u|(cp>>18));s[(*n)++]=(uint8_t)(0x80u|((cp>>12)&63u));s[(*n)++]=(uint8_t)(0x80u|((cp>>6)&63u));s[(*n)++]=(uint8_t)(0x80u|(cp&63u));}return 1;
}
static void hex(const uint8_t *p,size_t n){putchar('"');for(size_t i=0;i<n;i++)printf("%02x",p[i]);putchar('"');}
static uint32_t color_value(GhosttyStyleColor c){if(c.tag==GHOSTTY_STYLE_COLOR_PALETTE)return c.value.palette;if(c.tag==GHOSTTY_STYLE_COLOR_RGB)return ((uint32_t)c.value.rgb.r<<16)|((uint32_t)c.value.rgb.g<<8)|c.value.rgb.b;return 0;}
/* All three checked public results participate. Semantic width alone never identifies kind. */
static int kind(GhosttyCellWide wide,uint8_t semantic,bool text){
 switch(wide){case GHOSTTY_CELL_WIDE_NARROW:if(semantic==0)return text?5:6;if(semantic==1)return text?1:0;if(semantic==2&&text)return 7;return -1;
 case GHOSTTY_CELL_WIDE_WIDE:return semantic==2?2:-1;case GHOSTTY_CELL_WIDE_SPACER_TAIL:return semantic==0?3:-1;
 case GHOSTTY_CELL_WIDE_SPACER_HEAD:return semantic==1?4:-1;default:return -1;}
}
typedef struct {uint32_t cp[MAX_CP];size_t n;uint8_t text[4*MAX_CP],uri[MAX_URI];size_t text_n,uri_n;GhosttyStyle style;uint32_t bg_value;uint8_t bg_tag,semantic;GhosttyCellWide wide;bool has_text,has_link;int kind;} Cell;
static int cell(GhosttyTerminal t,uint32_t y,uint32_t x,Cell *c){
 CHECK(x<MAX_COLS&&y<MAX_ROWS);memset(c,0,sizeof(*c));
 GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=(uint16_t)x,.y=y}};GhosttyGridRef ref={0};GhosttyCell raw=0;
 CHECK(ghostty_terminal_grid_ref(t,p,&ref)==0);CHECK(ghostty_grid_ref_cell(&ref,&raw)==0);
 CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_WIDE,&c->wide)==0);CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_HAS_TEXT,&c->has_text)==0);
 CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_HAS_HYPERLINK,&c->has_link)==0);
 CHECK(ghostty_shellfox_terminal_cell_width(t,2,raw,&c->semantic)==0);c->kind=kind(c->wide,c->semantic,c->has_text);CHECK(c->kind>=0);kinds_seen|=1u<<(unsigned)c->kind;
 CHECK(ghostty_grid_ref_graphemes(&ref,c->cp,MAX_CP,&c->n)==0&&c->n<=MAX_CP);for(size_t i=0;i<c->n;i++)CHECK(utf8(c->cp[i],c->text,&c->text_n));
 CHECK(ghostty_grid_ref_hyperlink_uri(&ref,c->uri,sizeof(c->uri),&c->uri_n)==0&&c->uri_n<=MAX_URI);
 CHECK(ghostty_grid_ref_style(&ref,&c->style)==0);c->bg_tag=c->style.bg_color.tag;c->bg_value=color_value(c->style.bg_color);
 GhosttyCellContentTag tag;CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_CONTENT_TAG,&tag)==0);
 /* Inline empty-cell background is public cell content, not GhosttyStyle.bg. */
 if(tag==GHOSTTY_CELL_CONTENT_BG_COLOR_PALETTE){GhosttyColorPaletteIndex v;CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_COLOR_PALETTE,&v)==0);c->bg_tag=1;c->bg_value=v;}
 else if(tag==GHOSTTY_CELL_CONTENT_BG_COLOR_RGB){GhosttyColorRgb v;CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_COLOR_RGB,&v)==0);c->bg_tag=2;c->bg_value=((uint32_t)v.r<<16)|((uint32_t)v.g<<8)|v.b;}
 return 0;
}
static int capture(GhosttyTerminal t,unsigned index,unsigned pass){
 GhosttyShellfoxLayout2ScreenData s={.size=sizeof(s),.abi_version=1,.cell_layout_version=2};CHECK(ghostty_shellfox_terminal_layout2_screen_get(t,&s)==0);
 uint16_t cols=0,rows=0,x=0,y=0;bool pending=false;
 CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_COLS,&cols)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_ROWS,&rows)==0);
 CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_X,&x)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_Y,&y)==0);CHECK(ghostty_terminal_get(t,GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP,&pending)==0);
 CHECK(cols&&cols<=MAX_COLS&&rows&&rows<=MAX_ROWS&&s.total_rows>=rows&&s.total_rows<=MAX_ROWS&&s.physical_columns<=MAX_COLS);
 uint32_t logical=(uint32_t)x+(uint32_t)pending+s.cursor_virtual_overflow;
 printf("{\"case\":%u,\"pass\":%u,\"cols\":%u,\"rows\":%u,\"physical\":%u,\"total\":%"PRIu64",\"cursor\":[%u,%u,%u,%u,%u],\"saved\":[%u,%u,%u,%u,%u],\"data\":[",index,pass,cols,rows,s.physical_columns,s.total_rows,x,y,(unsigned)pending,s.cursor_virtual_overflow,logical,s.saved_present,s.saved_x,s.saved_y,s.saved_pending_wrap,s.saved_virtual_overflow);
 for(uint32_t row=0;row<s.total_rows;row++){
  GhosttyShellfoxLayout2RowData r={.size=sizeof(r),.abi_version=1,.cell_layout_version=2};GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=0,.y=row}};
  CHECK(ghostty_shellfox_terminal_layout2_row_get(t,p,&r)==0&&r.backing_columns<=s.physical_columns);Cell values[MAX_COLS];
  uint32_t limit=0;for(uint32_t col=0;col<r.backing_columns;col++){CHECK(cell(t,row,col,&values[col])==0);if(values[col].has_text||values[col].bg_tag)limit=col+values[col].semantic;}
  uint32_t no_bg=limit;bool cursor_row=row==s.total_rows-rows+y;uint32_t cursor_x=logical<cols?logical:cols-1u;
  if(cursor_row&&limit<cursor_x+1u)limit=cursor_x+1u;
  printf("%s{\"backing\":%u,\"trim\":%u,\"limit\":%u,\"cells\":[",row?",":"",r.backing_columns,no_bg,limit);
  uint32_t flow=0;for(uint32_t col=0;col<r.backing_columns;col++){Cell *c=&values[col];unsigned emit=col<limit&&c->semantic!=0;
   printf("%s{\"x\":%u,\"kind\":%d,\"stride\":1,\"advance\":%u,\"semantic\":%u,\"flow\":%u,\"emit\":%u,\"cursor_class\":%u,\"text\":",col?",":"",col,c->kind,c->wide==GHOSTTY_CELL_WIDE_WIDE?2u:1u,c->semantic,flow,emit,(unsigned)(emit&&cursor_row&&col==cursor_x));hex(c->text,c->text_n);
   printf(",\"uri\":");hex(c->uri,c->uri_n);printf(",\"has_link\":%u,\"bg\":[%u,%u],\"style\":[%u,%u,%u,%u,%u,%u,%u]}",(unsigned)c->has_link,c->bg_tag,c->bg_value,c->style.flags,c->style.fg_color.tag,color_value(c->style.fg_color),c->style.bg_color.tag,color_value(c->style.bg_color),c->style.underline_color.tag,color_value(c->style.underline_color));if(emit)flow+=c->semantic;
  }printf("]}");
 }puts("]}");return 0;
}
static int load(const char *dir,const char *name,const GhosttyShellfoxSnapshotFormat2Options *o,GhosttyTerminal *t){
 char path[4096];int n=snprintf(path,sizeof(path),"%s/%s.bin",dir,name);CHECK(n>0&&(size_t)n<sizeof(path));FILE *f=fopen(path,"rb");CHECK(f);CHECK(fseek(f,0,SEEK_END)==0);long length=ftell(f);CHECK(length>0&&(unsigned long)length<=MAX_BLOB);rewind(f);uint8_t *bytes=malloc((size_t)length);CHECK(bytes);CHECK(fread(bytes,1,(size_t)length,f)==(size_t)length&&fclose(f)==0);GhosttyShellfoxSnapshotFormat2Report report={.size=sizeof(report),.abi_version=1};int result=ghostty_shellfox_snapshot_format2_decode_buf(bytes,(size_t)length,o,t,&report);free(bytes);if(result)fprintf(stderr,"seed %s decode%d heap%"PRIu64" mapped%"PRIu64" work%"PRIu64"\n",name,result,report.heap_bytes,report.mapped_bytes,report.work_units);CHECK(result==0);return 0;
}
static int invalid(GhosttyTerminal t){
 GhosttyShellfoxLayout2ScreenData s={.size=sizeof(s),.abi_version=1,.cell_layout_version=2,.physical_columns=999};
 GhosttyShellfoxLayout2RowData r={.size=sizeof(r),.abi_version=1,.cell_layout_version=2,.backing_columns=999};
 for(unsigned i=0;i<4;i++){GhosttyShellfoxLayout2ScreenData a=s;GhosttyShellfoxLayout2RowData b=r;if(i==0){a.size--;b.size--;}if(i==1){a.abi_version++;b.abi_version++;}if(i==2){a.cell_layout_version--;b.cell_layout_version--;}if(i==3){a.reserved[1]=1;b.reserved[0]=1;}GhosttyShellfoxLayout2ScreenData ac=a;GhosttyShellfoxLayout2RowData bc=b;
 CHECK(ghostty_shellfox_terminal_layout2_screen_get(t,&a)==GHOSTTY_INVALID_VALUE&&memcmp(&a,&ac,sizeof(a))==0);GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_ACTIVE};CHECK(ghostty_shellfox_terminal_layout2_row_get(t,p,&b)==GHOSTTY_INVALID_VALUE&&memcmp(&b,&bc,sizeof(b))==0);}
 GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_ACTIVE,.value.coordinate={.x=1,.y=0}};GhosttyShellfoxLayout2RowData copy=r;CHECK(ghostty_shellfox_terminal_layout2_row_get(t,p,&r)==GHOSTTY_INVALID_VALUE&&memcmp(&r,&copy,sizeof(r))==0);return 0;
}
int main(int argc,char **argv){
 CHECK(argc==4);GhosttyShellfoxSnapshotFormat2Options o=opts(argv[2]);o.max_rows=MAX_ROWS;o.max_visible_columns=MAX_COLS;o.max_backing_columns=MAX_COLS;o.max_visible_rows=MAX_ROWS;o.max_physical_slots=MAX_ROWS*MAX_COLS;o.max_work_units=UINT64_C(1000000000000000);o.max_allocation_bytes=64u*1024u*1024u;
 GhosttyShellfoxTerminalProfileOptions live={.size=sizeof(live),.abi_version=1,.engine_profile=o.engine_profile,.terminal_profile=1,.cols=8,.rows=3};GhosttyTerminal t=NULL;CHECK(ghostty_shellfox_terminal_new_profile(NULL,&t,&live)==GHOSTTY_UNSUPPORTED&&t==NULL);
 if(strcmp(argv[1],"off")==0){GhosttyShellfoxLayout2ScreenData s={.size=sizeof(s),.abi_version=1,.cell_layout_version=2,.physical_columns=999};GhosttyShellfoxLayout2ScreenData saved=s;GhosttyShellfoxLayout2RowData r={.size=sizeof(r),.abi_version=1,.cell_layout_version=2,.backing_columns=999};GhosttyShellfoxLayout2RowData prior=r;GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_ACTIVE};CHECK(ghostty_shellfox_terminal_layout2_screen_get(NULL,&s)==GHOSTTY_UNSUPPORTED&&memcmp(&s,&saved,sizeof(s))==0);CHECK(ghostty_shellfox_terminal_layout2_row_get(NULL,p,&r)==GHOSTTY_UNSUPPORTED&&memcmp(&r,&prior,sizeof(r))==0);puts("defaultOFF public metadata refusal passed");return 0;}
 CHECK(strcmp(argv[1],"experimental")==0);
 GhosttyTerminal native=NULL;CHECK(ghostty_terminal_new(NULL,&native,8,3)==0);GhosttyShellfoxLayout2ScreenData native_data={.size=sizeof(native_data),.abi_version=1,.cell_layout_version=2,.physical_columns=999};GhosttyShellfoxLayout2ScreenData native_before=native_data;CHECK(ghostty_shellfox_terminal_layout2_screen_get(native,&native_data)==GHOSTTY_UNSUPPORTED&&memcmp(&native_data,&native_before,sizeof(native_data))==0);ghostty_terminal_free(native);
 uint8_t *blob=malloc(MAX_BLOB),*before=malloc(MAX_BLOB);CHECK(blob&&before);
 for(unsigned i=0;i<sizeof(cases)/sizeof(cases[0]);i++){const Case *c=&cases[i];GhosttyTerminal a=NULL,b=NULL;const char *seed=c->seed==1?"stub":c->seed==2?"tail":"empty";CHECK(load(argv[3],seed,&o,&a)==0);CHECK(ghostty_terminal_resize(a,c->cols,3,0,0)==0);
  for(size_t j=0;j<c->count;j++){if(c->steps[j].resize)CHECK(ghostty_terminal_resize(a,c->steps[j].resize,3,0,0)==0);else ghostty_terminal_vt_write(a,c->steps[j].text,c->steps[j].len);}
  char cursor[32];int n=snprintf(cursor,sizeof(cursor),"\033[%u;%uH",c->cursor_x<0?2u:1u,c->cursor_x<0?1u:(unsigned)c->cursor_x+1u);CHECK(n>0&&(size_t)n<sizeof(cursor));ghostty_terminal_vt_write(a,(const uint8_t*)cursor,(size_t)n);
  CHECK(invalid(a)==0);size_t previous=0;CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,before,MAX_BLOB,&previous,NULL)==0);CHECK(capture(a,i,0)==0);size_t len=0;CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,blob,MAX_BLOB,&len,NULL)==0);CHECK(len==previous&&memcmp(blob,before,len)==0);CHECK(len<=MAX_BLOB);GhosttyShellfoxSnapshotFormat2Report dr={.size=sizeof(dr),.abi_version=1};int decoded=ghostty_shellfox_snapshot_format2_decode_buf(blob,len,&o,&b,&dr);if(decoded)fprintf(stderr,"case%u decode%d work%"PRIu64"\n",i,decoded,dr.work_units);CHECK(decoded==0);CHECK(capture(b,i,1)==0);ghostty_terminal_free(a);ghostty_terminal_free(b);
 }
 /* Source cursor may be cols+1. Save/restore keeps the exact overflow, not a clamped guess. */
 GhosttyTerminal a=NULL,b=NULL;CHECK(load(argv[3],"empty",&o,&a)==0);CHECK(ghostty_terminal_resize(a,4,3,0,0)==0);const uint8_t overflow[]="ABCD\033[31m\xcc\x81";ghostty_terminal_vt_write(a,overflow,sizeof(overflow)-1);
 GhosttyShellfoxLayout2ScreenData s={.size=sizeof(s),.abi_version=1,.cell_layout_version=2};CHECK(ghostty_shellfox_terminal_layout2_screen_get(a,&s)==0&&s.cursor_virtual_overflow==1);
 ghostty_terminal_vt_write(a,(const uint8_t*)"\0337",2);s.size=sizeof(s);CHECK(ghostty_shellfox_terminal_layout2_screen_get(a,&s)==0&&s.saved_present==1&&s.saved_x==3&&s.saved_pending_wrap==1&&s.saved_virtual_overflow==1);
 ghostty_terminal_vt_write(a,(const uint8_t*)"\0338",2);CHECK(capture(a,(unsigned)(sizeof(cases)/sizeof(cases[0])),0)==0);size_t len=0;CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,blob,MAX_BLOB,&len,NULL)==0);CHECK(ghostty_shellfox_snapshot_format2_decode_buf(blob,len,&o,&b,NULL)==0);CHECK(capture(b,(unsigned)(sizeof(cases)/sizeof(cases[0])),1)==0);
 /* Known P4 live-cursor gap is rejected, never repaired by a metadata getter. */
 ghostty_terminal_vt_write(a,(const uint8_t*)"\033[2;2H",6);GhosttyShellfoxLayout2ScreenData prior=s;CHECK(ghostty_shellfox_terminal_layout2_screen_get(a,&s)==GHOSTTY_INVALID_VALUE&&memcmp(&s,&prior,sizeof(s))==0);
 ghostty_terminal_free(a);ghostty_terminal_free(b);a=NULL;b=NULL;
 CHECK(load(argv[3],"empty",&o,&a)==0);const uint8_t managed[]="\033[1;3;4;38;2;12;34;56;48;2;90;100;110m\033]8;;https://example.invalid/managed\033\\A\xcc\x81\033]8;;\033\\";ghostty_terminal_vt_write(a,managed,sizeof(managed)-1);Cell c;CHECK(cell(a,0,0,&c)==0&&c.n==2&&c.cp[0]=='A'&&c.cp[1]==0x301u&&c.bg_tag==2&&c.bg_value==0x5a646eu);CHECK((c.style.flags&GHOSTTY_STYLE_FLAG_BOLD)&&(c.style.flags&GHOSTTY_STYLE_FLAG_ITALIC)&&c.has_link);
 CHECK(capture(a,(unsigned)(sizeof(cases)/sizeof(cases[0]))+1u,0)==0);len=0;CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,blob,MAX_BLOB,&len,NULL)==0);CHECK(ghostty_shellfox_snapshot_format2_decode_buf(blob,len,&o,&b,NULL)==0);CHECK(capture(b,(unsigned)(sizeof(cases)/sizeof(cases[0]))+1u,1)==0);ghostty_terminal_free(a);ghostty_terminal_free(b);
 free(before);free(blob);fprintf(stderr,"kind-mask=%02x cases=%zu public-only source/encode/decode/metadata passed\n",kinds_seen,sizeof(cases)/sizeof(cases[0]));CHECK(kinds_seen==255u);return 0;
}
