#import <XCTest/XCTest.h>
#import "../SFTerminalView.h"

@interface SFTerminalView (Tests)
- (void)displayTick;
@end
@interface SFTerminalViewTests : XCTestCase
@end
@implementation SFTerminalViewTests
// A foreground rebind finds the terminal parser already ready. JS treats
// "bound" as not ready, so "ready" must follow it, never precede it.
- (void)testRebindingAReadyTerminalReportsReadyAfterBound {
  SFOwner *owner = [SFOwner shared];
  __block SFTerminal *terminal = nil;
  XCTestExpectation *created = [self expectationWithDescription:@"ready terminal"];
  XCTAssertTrue([owner enqueue:^{
    SFTerminal *value = nil;
    XCTAssertEqual([owner create:20 rows:5 result:&value], SFM_OK);
    value.mailbox->active.store(false);
    value.mailbox->ready.store(true);
    terminal = value;
    [created fulfill];
  }]);
  [self waitForExpectations:@[created] timeout:3];

  UIWindow *window = [[UIWindow alloc] initWithFrame:CGRectMake(0, 0, 320, 200)];
  SFTerminalView *view = [[SFTerminalView alloc] initWithFrame:window.bounds];
  NSMutableArray<NSString *> *states = [NSMutableArray new];
  XCTestExpectation *bound = [self expectationWithDescription:@"bound"];
  view.stateChanged = ^(NSString *state) {
    [states addObject:state];
    if ([state isEqual:@"bound"]) [bound fulfill];
  };
  [window addSubview:view];
  view.terminalHandle = terminal.identifier;
  // Hold the main queue until the owner has accepted the bind, so its
  // "bound" callback is still queued when the display link ticks.
  dispatch_semaphore_t accepted = dispatch_semaphore_create(0);
  XCTAssertTrue([owner enqueue:^{ dispatch_semaphore_signal(accepted); }]);
  XCTAssertEqual(dispatch_semaphore_wait(accepted, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)), 0);
  [view displayTick];
  [self waitForExpectations:@[bound] timeout:3];
  [view displayTick];
  XCTAssertEqualObjects(states, (@[@"bound", @"ready"]));

  [view removeFromSuperview];
  XCTestExpectation *closed = [self expectationWithDescription:@"terminal closed"];
  XCTAssertTrue([owner enqueue:^{
    XCTAssertEqual([owner close:terminal.identifier], SFM_OK);
    [closed fulfill];
  }]);
  [self waitForExpectations:@[closed] timeout:3];
}
@end
