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
- (void)testV1FilesRunDuringPollWithoutOvertakingQueuedWork {
  SFPollFixtureStore *store = [SFPollFixtureStore new];
  store.fixture = [SFClientRecord new];
  store.fixture.polling = YES;
  store.fixture.pollOperation = @"19";
  [store whenDrained:@"1" admission:SFClientAdmissionV1FileRequest work:^id(SfmStatus *status) {
    return @"20";
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_OK); XCTAssertEqualObjects(value, @"20"); }];
  XCTAssertEqualObjects(store.fixture.pollOperation, @"19");
  XCTAssertTrue([store.fixture.mutations containsObject:@"20"]);
  [store whenDrained:@"1" admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    XCTFail(@"Terminal work overtook an in-flight poll"); return nil;
  } done:^(id value, SfmStatus status) { XCTFail(@"Unexpected terminal completion"); }];
  [store completed:@{@"operationId": @"20", @"status": @(SFM_OK)} client:store.fixture];
  [store whenDrained:@"1" admission:SFClientAdmissionV1FileRequest work:^id(SfmStatus *status) {
    XCTFail(@"Files work overtook a queued terminal operation"); return nil;
  } done:^(id value, SfmStatus status) { XCTFail(@"Unexpected Files completion"); }];
  XCTAssertEqual(store.fixture.pendingCalls.count, 2u);
}
- (void)testFailedV1PollAllowsOnlyLocalRetirementAndAcceptedReconnectFence {
  SFPollFixtureStore *store = [self storeWithFailedPoll];
  [store whenDrained:@"1" admission:SFClientAdmissionOrdinary work:^id(SfmStatus *status) {
    XCTFail(@"Ordinary work entered an uncertain poll lane"); return nil;
  } done:^(id value, SfmStatus status) { XCTAssertEqual(status, SFM_ENGINE); }];
  [store whenDrained:@"1" admission:SFClientAdmissionV1FileRequest work:^id(SfmStatus *status) {
    XCTFail(@"Files work entered an uncertain poll lane"); return nil;
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
