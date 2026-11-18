#import <XCTest/XCTest.h>
#import "SFClientTestSupport.h"

@interface SFClientStore (ObserverTests)
- (void)finishClose:(SFClientRecord *)client;
@end
static NSString *const ObserverClientId = @"20000000-0000-4000-8000-000000000001";
static NSString *const ObserverTab = @"50000000-0000-4000-8000-000000000001";
static NSString *Golden(Class type, NSString *name) {
  NSString *file = [@"observer-v2-" stringByAppendingString:name];
  NSString *path = [[NSBundle bundleForClass:type] pathForResource:file ofType:@"json"] ?:
    [NSBundle.mainBundle pathForResource:file ofType:@"json"];
  return path ? [NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:nil] : nil;
}
static NSData *Encoded(id object) {
  return [NSJSONSerialization dataWithJSONObject:object options:0 error:nil];
}
@interface SFObserverWire : NSObject
@property(nonatomic) Class fixtureClass;
- (SFGoHTTPResponse *)request:(NSString *)path body:(NSString *)body;
- (NSDictionary *)lastAck;
- (NSUInteger)ackCount;
@end
@implementation SFObserverWire {
  NSLock *_lock;
  NSMutableArray<NSDictionary *> *_acks;
}
- (instancetype)init { if ((self = [super init])) { _lock = [NSLock new]; _acks = [NSMutableArray new]; } return self; }
- (NSDictionary *)lastAck { [_lock lock]; NSDictionary *ack = _acks.lastObject; [_lock unlock]; return ack; }
- (NSUInteger)ackCount { [_lock lock]; NSUInteger count = _acks.count; [_lock unlock]; return count; }
- (SFGoHTTPResponse *)request:(NSString *)path body:(NSString *)body {
  NSString *name = nil;
  if ([path isEqual:@"/v2/discover"]) name = @"discovery";
  else if ([path isEqual:@"/v2/pair"]) name = @"pair-selection";
  else if ([path isEqual:@"/v2/events"]) name = @"events";
  else if ([path isEqual:@"/v2/request"]) {
    id command = [NSJSONSerialization JSONObjectWithData:[body dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
    if ([command[@"method"] isEqual:@"attachTerminal"]) name = @"attachment";
    else if ([command[@"method"] isEqual:@"acknowledgeTerminal"]) {
      [_lock lock]; [_acks addObject:command[@"payload"][@"ack"]]; [_lock unlock]; name = @"acknowledged";
    }
  }
  NSString *response = name ? Golden(self.fixtureClass, name) : nil;
  return response ? [[SFGoHTTPResponse alloc] initWithStatusCode:200 body:response] : nil;
}
@end

@interface SFObserverV2Tests : XCTestCase
@end
@implementation SFObserverV2Tests
- (void)testObserverCanonicalGridAndConsumptionAcknowledgementsStayNative {
  XCTestExpectation *done = [self expectationWithDescription:@"observer canonical consumption"];
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFOwner *owner = [SFOwner shared]; SFClientStore *store = [SFClientStore shared];
    SFObserverWire *wire = [SFObserverWire new]; wire.fixtureClass = self.class;
    SFGoTransport *transport = [[SFGoTransport alloc] initWithRequestFunction:
      ^NSString *(NSString *peer, NSString *path, NSString *body, NSError **error) {
        XCTFail(@"Observer must not use legacy string/error transport"); return nil;
      } statusRequestFunction:^SFGoHTTPResponse *(NSString *peer, NSString *path, NSString *body, NSError **error) {
        return [wire request:path body:body];
      }];
    NSString *identifier = nil; SFClientRecord *client = nil; SFTerminal *terminal = nil;
    @try {
      SfmStatus status = [store createObserverPeer:@"n-fake-peer" clientId:ObserverClientId result:&identifier];
      XCTAssertEqual(status, SFM_OK); if (status != SFM_OK) return;
      client = [store client:identifier];
      uint32_t profile = 0, caps = 0;
      XCTAssertEqual(sfm_client_profile(client.handle, &profile, &caps), SFM_OK);
      XCTAssertEqual(profile, SFM_CLIENT_PROFILE_OBSERVER_V2); XCTAssertEqual(caps, SFM_CLIENT_API_DELIVERY);
      uint64_t operation = 0;
      XCTAssertEqual(sfm_observer_v2_discover(client.handle, 0, &operation), SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      NSData *attempt = [@"30000000-0000-4000-8000-000000000001" dataUsingEncoding:NSUTF8StringEncoding];
      XCTAssertEqual(sfm_observer_v2_pair(client.handle, (const uint8_t *)attempt.bytes, attempt.length, &operation), SFM_OK);
      NSDictionary *paired = SFTestClientEvent(client.handle, transport);
      XCTAssertTrue(SFTestClientSuccess(paired));
      XCTAssertEqualObjects(paired[@"profile"], @"observer-v2");
      XCTAssertNil(paired[@"connection"][@"permission"]); XCTAssertNil(paired[@"connection"][@"connectionId"]);
      XCTAssertEqual([owner create:20 rows:5 result:&terminal], SFM_OK);
      if (!terminal) return;
      XCTAssertEqual([store bind:identifier tab:ObserverTab terminal:terminal.identifier], SFM_OK);
      id envelope = [NSJSONSerialization JSONObjectWithData:[Golden(self.class, @"request-attach") dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
      NSData *command = Encoded(@{@"method":envelope[@"method"], @"payload":envelope[@"payload"]});
      XCTAssertEqual(sfm_observer_v2_operation(client.handle, (const uint8_t *)command.bytes, command.length, &operation), SFM_OK);
      NSDictionary *attached = SFTestClientEvent(client.handle, transport);
      XCTAssertTrue(SFTestClientSuccess(attached));
      XCTAssertEqualObjects(attached[@"result"][@"value"][@"kind"], @"attached");
      XCTAssertEqualObjects(attached[@"result"][@"value"][@"input"], NSNull.null);
      XCTAssertNil(attached[@"result"][@"value"][@"control"]);
      XCTAssertNil(attached[@"result"][@"value"][@"recovery"][@"data"]);
      // A caller's asserted cursor cannot grant credit before the parser consumes.
      XCTAssertNotEqual([store consumedTerminal:terminal.identifier sequence:UINT64_MAX], SFM_OK);
      XCTAssertEqual(wire.ackCount, 0u);
      uint64_t ticket = 0; uint16_t cols = 0, rows = 0;
      XCTAssertEqual([client functions].resetTicket(client.handle, terminal.handle, &ticket), SFM_OK);
      XCTAssertEqual([client functions].resetGrid(client.handle, terminal.handle, ticket, &cols, &rows), SFM_OK);
      XCTAssertEqual(cols, 80u); XCTAssertEqual(rows, 24u);
      XCTAssertEqual(sfm_terminal_reset(terminal.handle, cols, rows), SFM_OK);
      terminal.columns = cols; terminal.rows = rows;
      XCTAssertEqual([client functions].resetDone(client.handle, terminal.handle, ticket), SFM_OK);
      XCTAssertNotEqual([store consumedTerminal:terminal.identifier sequence:UINT64_MAX], SFM_OK);
      XCTAssertEqual(wire.ackCount, 0u);
      XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
      uint8_t ready = 0;
      XCTAssertEqual([store recoveryReady:terminal.identifier ready:&ready], SFM_OK); XCTAssertEqual(ready, 1u);
      terminal.recovering = NO;
      XCTAssertEqual([owner resize:terminal.identifier columns:10 rows:5], SFM_OK);
      XCTAssertEqual(terminal.columns, 80u); XCTAssertEqual(terminal.rows, 24u);
      NSDictionary *input = @{@"type":@"commit", @"text":@"never sent"};
      XCTAssertEqual([owner input:terminal.identifier event:input], SFM_INVALID);
      XCTAssertEqual([store consumedTerminal:terminal.identifier sequence:UINT64_MAX], SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      XCTAssertEqualObjects([wire lastAck][@"kind"], @"recovery"); XCTAssertEqualObjects([wire lastAck][@"sequence"], @4);
      XCTAssertEqual([store poll:identifier operation:&operation], SFM_OK);
      NSDictionary *events = SFTestClientEvent(client.handle, transport);
      XCTAssertTrue(SFTestClientSuccess(events));
      XCTAssertEqual([events[@"result"][@"value"][@"events"] count], 1u);
      XCTAssertEqualObjects([events[@"result"][@"value"][@"events"] firstObject][@"event"][@"type"], @"sourceEnded");
      XCTAssertEqual([store consumedTerminal:terminal.identifier sequence:UINT64_MAX], SFM_CLIENT_EMPTY);
      XCTAssertEqual(wire.ackCount, 1u);
      XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
      XCTAssertEqual([store consumedTerminal:terminal.identifier sequence:UINT64_MAX], SFM_OK);
      XCTAssertTrue(SFTestClientSuccess(SFTestClientEvent(client.handle, transport)));
      XCTAssertEqualObjects([wire lastAck][@"kind"], @"data"); XCTAssertEqualObjects([wire lastAck][@"sequence"], @5);
    } @finally {
      if (terminal && identifier) [store unbind:identifier terminal:terminal.identifier];
      if (client) { [transport closeClient:client.handle]; [store finishClose:client]; }
      if (terminal) [owner close:terminal.identifier];
      [done fulfill];
    }
  }];
  XCTAssertTrue(accepted); [self waitForExpectations:@[done] timeout:20];
}
- (void)testOnlyActualDiscover404OffersLegacyAndNeverPairsAutomatically {
  XCTestExpectation *done = [self expectationWithDescription:@"status carrier discovery"];
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFClientStore *store = [SFClientStore shared];
    for (NSNumber *scenario in @[@404, @401, @0]) {
      NSLock *lock = [NSLock new]; NSMutableArray<NSString *> *paths = [NSMutableArray new];
      SFGoTransport *transport = [[SFGoTransport alloc] initWithRequestFunction:
        ^NSString *(NSString *peer, NSString *path, NSString *body, NSError **error) {
          XCTFail(@"Observer cannot downgrade its native carrier"); return nil;
        } statusRequestFunction:^SFGoHTTPResponse *(NSString *peer, NSString *path, NSString *body, NSError **error) {
          [lock lock]; [paths addObject:path]; [lock unlock];
          if ([path isEqual:@"/v1/discover"]) return [[SFGoHTTPResponse alloc] initWithStatusCode:200
            body:@"{\"ok\":true,\"value\":{\"service\":\"shellfox\",\"protocolVersion\":1}}"];
          if (scenario.intValue == 0) {
            if (error) *error = [NSError errorWithDomain:@"fixture" code:404
              userInfo:@{NSLocalizedDescriptionKey:@"404 is diagnostic text, not HTTP status"}];
            return nil;
          }
          return [[SFGoHTTPResponse alloc] initWithStatusCode:scenario.longValue body:@""];
        }];
      NSString *identifier = nil; SFClientRecord *client = nil;
      @try {
        SfmStatus status = [store createObserverPeer:@"n-fake-peer" clientId:ObserverClientId result:&identifier];
        XCTAssertEqual(status, SFM_OK); if (status != SFM_OK) continue;
        client = [store client:identifier]; uint64_t operation = 0;
        XCTAssertEqual(sfm_observer_v2_discover(client.handle, 1, &operation), SFM_OK);
        NSDictionary *event = SFTestClientEvent(client.handle, transport);
        [lock lock]; NSArray *requested = paths.copy; [lock unlock];
        if (scenario.intValue == 404) {
          XCTAssertTrue(SFTestClientSuccess(event));
          XCTAssertEqualObjects(event[@"result"][@"kind"], @"legacyAvailable");
          NSArray *expected = @[@"/v2/discover", @"/v1/discover"];
          XCTAssertEqualObjects(requested, expected);
        } else {
          XCTAssertNotNil(event); XCTAssertNotEqual([event[@"status"] intValue], SFM_OK);
          NSArray *expected = @[@"/v2/discover"]; XCTAssertEqualObjects(requested, expected);
        }
      } @finally {
        if (client) { [transport closeClient:client.handle]; [store finishClose:client]; }
      }
    }
    [done fulfill];
  }];
  XCTAssertTrue(accepted); [self waitForExpectations:@[done] timeout:20];
}
@end
