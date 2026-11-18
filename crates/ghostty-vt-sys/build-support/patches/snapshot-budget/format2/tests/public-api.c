/* Explicit experimental DLL boundary proof, not Legacy availability. */
#include <ghostty/vt/shellfox_snapshot_format2.h>
#include <ghostty/vt/formatter.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"line%d: %s\n",__LINE__,#x); return 1; } } while(0)
static GhosttyShellfoxSnapshotFormat2Options opts(const char *identity) {
  GhosttyShellfoxSnapshotFormat2Options o = {
    .size=sizeof(o), .abi_version=1, .format_version=2, .wire_profile=2,
    .layout_abi=2, .terminal_profile=1, .runtime_resize_pull_scrollback=1,
    .engine_profile={.ptr=(const uint8_t*)identity,.len=strlen(identity)},
    .max_encoded_bytes=4*1024*1024, .max_allocation_bytes=256*1024*1024,
    .max_work_units=UINT64_C(1000000000000000), .max_physical_slots=1000000,
    .max_rows=100000, .max_text_scalars=1000000, .max_text_bytes=4*1024*1024,
    .max_continuation_bytes=4096, .max_pages=1024, .max_records=4096,
    .max_visible_columns=4096, .max_backing_columns=4096, .max_visible_rows=4096,
    .max_pixel_width=65535, .max_pixel_height=65535
  };
  return o;
}
static int copy(GhosttyTerminal t, uint8_t *buf, size_t cap, size_t *len) {
  GhosttyFormatter f=NULL;
  if(ghostty_formatter_terminal_new(NULL,&f,t)!=GHOSTTY_SUCCESS) return 0;
  GhosttyResult r=ghostty_formatter_format_buf(f,buf,cap,len);
  ghostty_formatter_free(f);
  return r==GHOSTTY_SUCCESS;
}
int main(int argc,char **argv) {
  CHECK(argc==4);
  GhosttyShellfoxSnapshotFormat2Options o=opts(argv[2]);
  GhosttyShellfoxSnapshotFormat2Report report={.size=sizeof(report),.abi_version=1};
  GhosttyTerminal a=NULL,b=NULL;
  GhosttyShellfoxTerminalProfileOptions live={.size=sizeof(live),.abi_version=1,
    .engine_profile=o.engine_profile,.terminal_profile=1,.cols=8,.rows=3};
  CHECK(ghostty_shellfox_terminal_new_profile(NULL,&a,&live)==GHOSTTY_UNSUPPORTED&&a==NULL);
  uint8_t scratch[32]; memset(scratch,0xa5,sizeof(scratch));
  size_t n=99;
  if(strcmp(argv[1],"off")==0) {
    CHECK(ghostty_shellfox_snapshot_format2_encode_buf(NULL,&o,scratch,sizeof(scratch),&n,&report)==GHOSTTY_UNSUPPORTED);
    CHECK(n==0);
    CHECK(ghostty_shellfox_snapshot_format2_decode_buf(scratch,sizeof(scratch),&o,&a,&report)==GHOSTTY_UNSUPPORTED);
    CHECK(a==NULL);
    for(size_t i=0;i<sizeof(scratch);i++) CHECK(scratch[i]==0xa5);
    puts("format2 default-OFF C boundary passed"); return 0;
  }
  CHECK(strcmp(argv[1],"experimental")==0);
  FILE *f=fopen(argv[3],"rb"); CHECK(f!=NULL);
  CHECK(fseek(f,0,SEEK_END)==0); long length=ftell(f);
  CHECK(length>32&&length<=4*1024*1024&&fseek(f,0,SEEK_SET)==0);
  uint8_t *bytes=(uint8_t*)malloc((size_t)length); CHECK(bytes!=NULL);
  CHECK(fread(bytes,1,(size_t)length,f)==(size_t)length&&fclose(f)==0);
  CHECK(memcmp(bytes,"SFOXSN2\0",8)==0);
  CHECK(ghostty_shellfox_snapshot_format2_decode_buf(bytes,(size_t)length,&o,&a,&report)==GHOSTTY_SUCCESS);
  CHECK(a!=NULL&&report.physical_slots>0&&report.heap_bytes>0&&report.mapped_bytes>0);
  CHECK(report.heap_bytes+report.mapped_bytes<=o.max_allocation_bytes&&report.work_units<=o.max_work_units);
  GhosttyShellfoxTerminalProfileState state={.size=sizeof(state),.abi_version=1};
  CHECK(ghostty_shellfox_terminal_get_profile_state(a,&state)==GHOSTTY_SUCCESS&&state.terminal_profile==1);
  n=99;
  CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,scratch,1,&n,NULL)==GHOSTTY_OUT_OF_SPACE);
  CHECK(n==0); for(size_t i=0;i<sizeof(scratch);i++) CHECK(scratch[i]==0xa5);
  size_t saved_limit=o.max_encoded_bytes;
  CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,scratch,sizeof(scratch),&n,NULL)==GHOSTTY_OUT_OF_SPACE&&n==0);
  o.max_encoded_bytes=sizeof(scratch);
  CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,scratch,sizeof(scratch),&n,NULL)==GHOSTTY_LIMIT_EXCEEDED&&n==0);
  o.max_encoded_bytes=saved_limit;
  uint8_t *output=(uint8_t*)malloc(4*1024*1024); CHECK(output!=NULL);
  o.max_continuation_bytes=1;
  CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,output,4*1024*1024,&n,NULL)==GHOSTTY_LIMIT_EXCEEDED&&n==0);
  o.max_continuation_bytes=4096;
  CHECK(ghostty_shellfox_snapshot_format2_encode_buf(a,&o,output,4*1024*1024,&n,NULL)==GHOSTTY_SUCCESS);
  CHECK(ghostty_shellfox_snapshot_format2_decode_buf(output,n,&o,&b,NULL)==GHOSTTY_SUCCESS);
  const char *suffix="\033[2@\033[1P\033[2X\033[?1049l!";
  ghostty_terminal_vt_write(a,(const uint8_t*)suffix,strlen(suffix));
  ghostty_terminal_vt_write(b,(const uint8_t*)suffix,strlen(suffix));
  uint8_t ac[65536],bc[65536]; size_t an=0,bn=0;
  CHECK(copy(a,ac,sizeof(ac),&an)&&copy(b,bc,sizeof(bc),&bn));
  CHECK(an==bn&&memcmp(ac,bc,an)==0);
  GhosttyShellfoxTerminalProfileState second={.size=sizeof(second),.abi_version=1};
  state.size=sizeof(state); state.abi_version=1;
  CHECK(ghostty_shellfox_terminal_get_profile_state(a,&state)==GHOSTTY_SUCCESS);
  CHECK(ghostty_shellfox_terminal_get_profile_state(b,&second)==GHOSTTY_SUCCESS);
  CHECK(state.preceding_join==second.preceding_join);
  ghostty_terminal_free(a); ghostty_terminal_free(b); a=NULL;
  o.max_allocation_bytes=0;
  CHECK(ghostty_shellfox_snapshot_format2_decode_buf(bytes,(size_t)length,&o,&a,NULL)==GHOSTTY_LIMIT_EXCEEDED&&a==NULL);
  free(bytes); free(output);
  printf("format2 experimental C codec passed; options=%zu report=%zu\n",sizeof(o),sizeof(report));
  return 0;
}
