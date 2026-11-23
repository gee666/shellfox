#import <XCTest/XCTest.h>
#import "SFClientStore.h"

@interface SFPollFixtureStore : SFClientStore
@property(nonatomic, strong) SFClientRecord *fixture;
@end
@implementation SFPollFixtureStore
- (SFClientRecord *)client:(NSString *)identifier { return [identifier isEqual:@"1"] ? self.fixture : nil; }
@end

@interface SFClientPollLaneTests : XCTestCase
@end
@implementation SFClientPollLaneTests
- (SFPollFixtureStore *)storeWithFailedPoll {
  SFPollFixtureStore *store = [SFPollFixtureStore new];
  store.fixture = [SFClientRecord new];
  store.fixture.polling = YES; store.fixture.pollOperation = @"19";
  [store completed:@{@"operationId": @"19", @"status": @(SFM_ENGINE)} client:store.fixture];
  XCTAssertEqual(store.fixture.pollFailure, SFM_ENGINE);
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
  return store;
}
- (void)testOperationsRunAlongsideThePollOneAtATimeInOrder {
  SFPollFixtureStore *store = [SFPollFixtureStore new];
  store.fixture = [SFClientRecord new];
  store.fixture.polling = YES;
  store.fixture.pollOperation = @"19";
  NSMutableArray<NSString *> *started = [NSMutableArray new];
  // The completion below carries the same connection authority as before.
  store.fixture.authorityKey = @"[null,null]";
  [store whenDrained:@"1" admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    [started addObject:@"attach"]; return @"20";
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); }];
  [store whenDrained:@"1" admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    [started addObject:@"file"]; return @"21";
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); }];
  [store drainClient:store.fixture];
  XCTAssertEqualObjects(started, @[@"attach"]);
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
  [store completed:@{@"operationId": @"20", @"status": @(SFM_OK)} client:store.fixture];
  [store drainClient:store.fixture];
  XCTAssertEqualObjects(started, (@[@"attach", @"file"]));
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
}
- (void)testReconnectDoesNotWaitForTheOutstandingPoll {
  SFPollFixtureStore *store = [SFPollFixtureStore new];
  store.fixture = [SFClientRecord new];
  store.fixture.polling = YES;
  store.fixture.pollOperation = @"19";
  [store whenDrained:@"1" admission:SFClientAdmissionV1Reconnect work:^id(SfmStatus *status) {
    return @"20";
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); }];
  XCTAssertNil(store.fixture.pollOperation);
  XCTAssertTrue([store.fixture.mutations containsObject:@"20"]);
  [store completed:@{@"operationId": @"19", @"status": @(SFM_CLIENT_CANCELLED)} client:store.fixture];
  XCTAssertEqual(store.fixture.pollFailure, SFM_OK);
}
- (void)testFailedV1PollAllowsOnlyLocalRetirementAndAcceptedReconnectFence {
  SFPollFixtureStore *store = [self storeWithFailedPoll];
  [store whenDrained:@"1" admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    XCTFail(@"Ordinary work entered an uncertain poll lane"); return nil;
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_ENGINE); }];

  __block BOOL retired = NO;
  [store whenDrained:@"1" admission:SFClientAdmissionV1RetireView work:^id(SfmStatus *status) {
    retired = YES; return nil;
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); }];
  XCTAssertTrue(retired);
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
  XCTAssertEqual(store.fixture.pollFailure, SFM_ENGINE);

  [store whenDrained:@"1" admission:SFClientAdmissionV1Reconnect work:^id(SfmStatus *status) {
    *status = SFM_BUSY; return nil;
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_BUSY); }];
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
  XCTAssertEqual(store.fixture.pollFailure, SFM_ENGINE);

  NSUInteger oldEpoch = store.fixture.authorityEpoch;
  [store whenDrained:@"1" admission:SFClientAdmissionV1Reconnect work:^id(SfmStatus *status) {
    return @"20";
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); }];
  XCTAssertNil(store.fixture.pollOperation);
  XCTAssertEqual(store.fixture.pollFailure, SFM_OK);
  XCTAssertEqual(store.fixture.authorityEpoch, oldEpoch + 1);
  XCTAssertTrue([store.fixture.mutations containsObject:@"20"]);
  XCTAssertFalse(store.fixture.polling);
}
@end
