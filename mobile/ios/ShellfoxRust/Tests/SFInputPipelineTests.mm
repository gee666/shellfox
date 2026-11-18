#import <XCTest/XCTest.h>
#import "SFClientTestSupport.h"

@interface SFClientStore (PipelineTests)
- (void)finishClose:(SFClientRecord *)client;
@end

static NSString *const Tab = @"20000000-0000-4000-8000-000000000001";
static NSString *const Generation = @"30000000-0000-4000-8000-000000000001";
static NSString *JSON(id value) {
  NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
  return [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
}

// Replaces only the Go request function. Production copying, worker slots,
// completion tokens, Rust client, encoder and SFClientStore all still execute.
@interface SFMockGo : NSObject
- (NSString *)request:(NSString *)peer path:(NSString *)path body:(NSString *)body error:(NSError **)error;
- (NSDictionary *)lastWrite;
- (void)queueMouseModes;
@end
@implementation SFMockGo {
  NSLock *_lock;
  NSMutableArray<NSDictionary *> *_writes;
  NSArray *_events;
}
- (instancetype)init {
  if ((self = [super init])) { _lock = [NSLock new]; _writes = [NSMutableArray new]; _events = @[]; }
  return self;
}
- (NSDictionary *)lastWrite {
  [_lock lock]; NSDictionary *write = _writes.lastObject; [_lock unlock]; return write;
}
- (void)queueMouseModes {
  [_lock lock];
  _events = @[@{@"type":@"terminal", @"event":@{@"type":@"data", @"tabId":Tab,
    @"generation":Generation, @"controlEpoch":@1, @"sequence":@2,
    @"data":@"\x1b[?1003h\x1b[?1006h"}}];
  [_lock unlock];
}
- (NSString *)request:(NSString *)peer path:(NSString *)path body:(NSString *)body error:(NSError **)error {
  if (![peer isEqual:@"n-fake-peer"]) return nil;
  if ([path isEqual:@"/v1/pair"]) return JSON(@{@"ok":@YES,
    @"value":@{@"paired":@YES, @"terminalControl":@"exclusive-v1"}});
  if ([path isEqual:@"/v1/events"]) {
    [_lock lock]; NSArray *events = _events; _events = @[]; [_lock unlock];
    return JSON(@{@"ok":@YES, @"value":@{@"events":events}});
  }
  NSData *bytes = [body dataUsingEncoding:NSUTF8StringEncoding];
  id command = [NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];
  if (![path isEqual:@"/v1/request"] || ![command isKindOfClass:NSDictionary.class]) return nil;
  NSString *method = command[@"method"];
  if ([method isEqual:@"attachTerminal"]) return JSON(@{@"ok":@YES, @"value":@{
    @"control":@"owned", @"controlEpoch":@1, @"tabId":Tab,
    @"sessionId":@"40000000-0000-4000-8000-000000000001", @"generation":Generation,
    @"firstSequence":@1, @"lastSequence":@1,
    @"chunks":@[@{@"type":@"data", @"tabId":Tab, @"generation":Generation, @"sequence":@1, @"data":@"hello"}],
    @"truncated":@NO, @"snapshot":@YES, @"state":@"open", @"exitCode":NSNull.null,
    @"cols":@80, @"rows":@24, @"lifetime":@"app-owned"}});
  if ([method isEqual:@"acknowledgeTerminal"]) return JSON(@{@"ok":@YES, @"value":@{@"acknowledged":@YES}});
  if ([method isEqual:@"writeTerminal"]) {
    [_lock lock]; [_writes addObject:@{@"raw":body, @"body":command}]; [_lock unlock];
    return JSON(@{@"ok":@YES, @"value":@{@"written":@YES}});
  }
  return nil;
}
@end

@interface SFInputPipelineTests : XCTestCase
@end
@implementation SFInputPipelineTests
- (void)testCtrlSpaceNulAndPassiveHintReachGoBoundaryWithoutReencoding {
  XCTestExpectation *done = [self expectationWithDescription:@"native input through Go boundary"];
  BOOL queued = [[SFOwner shared] enqueue:^{
    SFOwner *owner = [SFOwner shared]; SFClientStore *store = [SFClientStore shared];
    SFMockGo *mock = [SFMockGo new];
    SFGoTransport *transport = [[SFGoTransport alloc] initWithRequestFunction:
      ^NSString *(NSString *peer, NSString *path, NSString *body, NSError **error) {
        return [mock request:peer path:path body:body error:error];
      }];
    NSString *identifier = nil; SFTerminal *terminal = nil; SFClientRecord *client = nil;
    @try {
      SfmStatus status = [store createPeer:@"n-fake-peer" clientId:@"10000000-0000-4000-8000-000000000001" result:&identifier];
      XCTAssertEqual(status, SFM_OK); if (status != SFM_OK) return;
      client = [store client:identifier];
      uint64_t operation = 0;
      status = sfm_client_connect(client.handle, &operation);
      XCTAssertEqual(status, SFM_OK); if (status != SFM_OK) return;
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      status = [owner create:80 rows:24 result:&terminal];
      XCTAssertEqual(status, SFM_OK); if (status != SFM_OK) return;
      XCTAssertEqual([store bind:identifier tab:Tab terminal:terminal.identifier], SFM_OK);
      NSData *attach = [JSON(@{@"method":@"attachTerminal", @"payload":@{@"tabId":Tab}}) dataUsingEncoding:NSUTF8StringEncoding];
      XCTAssertEqual(sfm_client_operation(client.handle, (const uint8_t *)attach.bytes, attach.length, &operation), SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      uint64_t ticket = 0; uint16_t cols = 0, rows = 0;
      XCTAssertEqual(sfm_client_terminal_reset_ticket(client.handle, terminal.handle, &ticket), SFM_OK);
      XCTAssertEqual(sfm_client_terminal_reset_grid(client.handle, terminal.handle, ticket, &cols, &rows), SFM_OK);
      XCTAssertEqual(sfm_terminal_reset(terminal.handle, cols, rows), SFM_OK);
      XCTAssertEqual(sfm_client_terminal_reset_done(client.handle, terminal.handle, ticket), SFM_OK);
      XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
      uint8_t ready = 0;
      XCTAssertEqual(sfm_client_terminal_recovery_ready(client.handle, terminal.handle, &ready), SFM_OK);
      XCTAssertEqual(ready, 1u);
      terminal.recovering = NO;
      XCTAssertEqual(sfm_client_terminal_consumed(client.handle, terminal.handle), SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      NSDictionary *ctrlSpace = @{@"type":@"key", @"key":@{@"kind":@"character", @"value":@" "},
        @"action":@"press", @"modifiers":@2};
      XCTAssertEqual([owner input:terminal.identifier event:ctrlSpace], SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      NSDictionary *write = [mock lastWrite];
      NSString *nul = write[@"body"][@"payload"][@"data"];
      XCTAssertEqual(nul.length, 1u);
      XCTAssertEqual([nul characterAtIndex:0], 0u);
      XCTAssertEqual([nul lengthOfBytesUsingEncoding:NSUTF8StringEncoding], 1u);
      XCTAssertEqualObjects(write[@"body"][@"payload"][@"userInitiated"], @YES);
      XCTAssertTrue([write[@"raw"] containsString:@"\\u0000"]);
      [mock queueMouseModes];
      XCTAssertEqual(sfm_client_poll(client.handle, &operation), SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
      XCTAssertEqual(sfm_client_terminal_consumed(client.handle, terminal.handle), SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      NSDictionary *hover = @{@"type":@"mouse", @"action":@"motion", @"button":NSNull.null,
        @"x":@16, @"y":@32, @"cellWidth":@8, @"cellHeight":@16, @"anyButtonPressed":@NO};
      XCTAssertEqual([owner input:terminal.identifier event:hover], SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      XCTAssertEqualObjects([mock lastWrite][@"body"][@"payload"][@"userInitiated"], @NO);
    } @finally {
      // All work stays inside one owner block, so the shared live-Go driver cannot
      // service this fixture. Close/remove both registries before returning.
      if (terminal && identifier) [store unbind:identifier terminal:terminal.identifier];
      if (client) { [transport closeClient:client.handle]; [store finishClose:client]; }
      if (terminal) [owner close:terminal.identifier];
      [done fulfill];
    }
  }];
  XCTAssertTrue(queued);
  [self waitForExpectations:@[done] timeout:20];
}
@end
