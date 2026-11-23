#import <XCTest/XCTest.h>
#import "../SFOwner.h"
#import "../SFTextInput.h"
#import "../SFClientStore.h"

@interface SFClientStore (NativeTests)
- (BOOL)shouldForward:(id)event client:(SFClientRecord *)client;
@end

static void SFTestAppendBytes(void *context, const uint8_t *bytes, size_t length) {
  [(__bridge NSMutableData *)context appendBytes:bytes length:length];
}
static void FrameForTest(void *context, const SfmFrame *frame) {
  auto *result = static_cast<std::shared_ptr<const shellfox::Frame> *>(context);
  *result = shellfox::Frame::copy(*frame);
}
@interface SFOwner (NativeTests)
- (SfmStatus)submitIntent:(SFNativeIntent *)intent terminal:(SFTerminal *)terminal;
- (SfmStatus)retryIntents:(SFTerminal *)terminal;
@end
@interface SFTestInputSink : NSObject <SFRemoteTerminalSink>
@property(nonatomic) BOOL busyResize;
@property(nonatomic, strong) NSMutableArray<NSString *> *sent;
@property(nonatomic, strong) NSMutableArray<NSNumber *> *hints;
@property(nonatomic, copy) void (^onConsumed)(uint64_t);
@end
@implementation SFTestInputSink
- (instancetype)init {
  if ((self = [super init])) { _sent = [NSMutableArray new]; _hints = [NSMutableArray new]; }
  return self;
}
- (SfmStatus)sendInput:(NSData *)bytes userInitiated:(uint8_t)hint terminal:(NSString *)terminal {
  [self.hints addObject:@(hint)];
  [self.sent addObject:[[NSString alloc] initWithData:bytes encoding:NSUTF8StringEncoding] ?: @"binary"]; return SFM_OK;
}
- (SfmStatus)resizeTerminal:(NSString *)terminal columns:(uint16_t)columns rows:(uint16_t)rows {
  if (self.busyResize) return SFM_BUSY;
  [self.sent addObject:@"resize"]; return SFM_OK;
}
- (SfmStatus)consumedTerminal:(NSString *)terminal sequence:(uint64_t)sequence {
  if (self.onConsumed) self.onConsumed(sequence);
  return SFM_CLIENT_EMPTY;
}
- (SfmStatus)recoveryReady:(NSString *)terminal ready:(uint8_t *)ready { *ready = 0; return SFM_OK; }
- (void)terminalFailed:(NSString *)terminal status:(SfmStatus)status {}
@end
@interface SFNativeTests : XCTestCase
@end
@implementation SFNativeTests
- (void)testOwnerIdentityAndBorrowedFrameCopy {
  XCTestExpectation *done = [self expectationWithDescription:@"owner round trip"];
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFTerminal *terminal = nil;
    XCTAssertEqual([[SFOwner shared] create:20 rows:5 result:&terminal], SFM_OK);
    XCTAssertTrue([[SFOwner shared] isOwnerThread]);
    const uint8_t bytes[] = "hello";
    XCTAssertEqual(sfm_terminal_submit(terminal.handle, 1, bytes, 5), SFM_OK);
    XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
    std::shared_ptr<const shellfox::Frame> frame;
    XCTAssertEqual(sfm_terminal_frame(terminal.handle, FrameForTest, &frame), SFM_OK);
    XCTAssertTrue(frame != nullptr);
    XCTAssertEqual(frame->consumed, 1u);
    XCTAssertTrue(frame->cells[0].text == "h");
    SfmHandle handle = terminal.handle;
    dispatch_semaphore_t checked = dispatch_semaphore_create(0);
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
      XCTAssertEqual(sfm_terminal_resize(handle, 21, 5), SFM_WRONG_THREAD);
      dispatch_semaphore_signal(checked);
    });
    XCTAssertEqual(dispatch_semaphore_wait(checked, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC)), 0);
    XCTAssertEqual(sfm_terminal_reset(handle, 20, 5), SFM_OK);
    SfmProgress progress = {};
    XCTAssertEqual(sfm_terminal_progress(handle, &progress), SFM_OK);
    XCTAssertEqual(progress.accepted_sequence, 0u);
    XCTAssertEqual(progress.consumed_sequence, 0u);
    XCTAssertEqual([[SFOwner shared] close:terminal.identifier], SFM_OK);
    // Borrowed C text has been freed. This immutable frame still owns its text.
    XCTAssertTrue(frame->cells[0].text == "h");
    [done fulfill];
  }];
  XCTAssertTrue(accepted);
  [self waitForExpectations:@[done] timeout:5];
}
- (void)testOffscreenBoundTerminalDrainsOutputWithoutPublishingAFrame {
  SFOwner *owner = [SFOwner shared];
  SFTestInputSink *sink = [SFTestInputSink new];
  XCTestExpectation *drained = [self expectationWithDescription:@"credit for offscreen output"];
  XCTestExpectation *closed = [self expectationWithDescription:@"offscreen owner cleanup"];
  __block SFTerminal *terminal = nil;
  __block BOOL notified = NO;
  sink.onConsumed = ^(uint64_t sequence) {
    if (sequence != 1 || notified) return;
    notified = YES;
    [drained fulfill];
  };
  XCTAssertTrue([owner enqueue:^{
    SFTerminal *created = nil;
    XCTAssertEqual([owner create:20 rows:5 result:&created], SFM_OK);
    terminal = created;
    terminal.sink = sink;
    terminal.visible = NO;
    terminal.mailbox->active.store(false);
    const uint8_t text[] = "offscreen";
    XCTAssertEqual(sfm_terminal_submit(terminal.handle, 1, text, sizeof(text) - 1), SFM_OK);
  }]);
  [self waitForExpectations:@[drained] timeout:3];
  XCTAssertTrue([owner enqueue:^{
    XCTAssertTrue(terminal.mailbox->take() == nullptr);
    terminal.sink = nil;
    // Clean up even if the earlier assertion timed out.
    XCTAssertEqual(sfm_terminal_pump(terminal.handle, nullptr, nullptr), SFM_OK);
    XCTAssertEqual([owner close:terminal.identifier], SFM_OK);
    [closed fulfill];
  }]);
  [self waitForExpectations:@[closed] timeout:3];
}
- (void)testMailboxReplacesPendingFrameAndRetainsDisplayedFrame {
  shellfox::Mailbox mailbox;
  auto first = std::make_shared<shellfox::Frame>(); first->consumed = 1;
  auto second = std::make_shared<shellfox::Frame>(); second->consumed = 2;
  mailbox.put(first); auto displayed = mailbox.take();
  mailbox.put(first); mailbox.put(second);
  auto next = mailbox.take();
  XCTAssertEqual(displayed->consumed, 1u);
  XCTAssertEqual(next->consumed, 2u);
  XCTAssertTrue(mailbox.take() == nullptr);
  mailbox.put(first); mailbox.resetFrame();
  XCTAssertTrue(mailbox.take() == nullptr);
  XCTAssertEqual(mailbox.epoch.load(), 1u);
}
- (void)testFrameRejectsMalformedDimensionsBeforeReadingCells {
  SfmFrame invalid = {}; invalid.cols = 513; invalid.rows = 1;
  XCTAssertTrue(shellfox::Frame::copy(invalid) == nullptr);
}
- (void)testUnacceptedIntentsPreservePressResizeReleaseWithoutDuplicateRetry {
  XCTestExpectation *done = [self expectationWithDescription:@"preadmission order"];
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFOwner *owner = [SFOwner shared]; SFTerminal *terminal = nil;
    XCTAssertEqual([owner create:20 rows:5 result:&terminal], SFM_OK);
    SFTestInputSink *sink = [SFTestInputSink new]; sink.busyResize = YES; terminal.sink = sink;
    SFNativeIntent *press = [SFNativeIntent new]; press.kind = SFIntentWrite; press.userInitiated = 1;
    press.data = [@"press" dataUsingEncoding:NSUTF8StringEncoding];
    XCTAssertEqual([owner submitIntent:press terminal:terminal], SFM_OK);
    XCTAssertEqual([owner resize:terminal.identifier columns:21 rows:5], SFM_OK);
    SFNativeIntent *release = [SFNativeIntent new]; release.kind = SFIntentRelease;
    release.data = [@"release" dataUsingEncoding:NSUTF8StringEncoding];
    XCTAssertEqual([owner submitIntent:release terminal:terminal], SFM_OK);
    [owner retryIntents:terminal];
    XCTAssertEqual(sink.sent.count, 1u);
    sink.busyResize = NO;
    [owner retryIntents:terminal]; [owner retryIntents:terminal];
    NSArray *expected = @[@"press", @"resize", @"release"];
    XCTAssertEqualObjects(sink.sent, expected);
    NSArray *expectedHints = @[@1, @0];
    XCTAssertEqualObjects(sink.hints, expectedHints);
    XCTAssertEqual(terminal.pendingIntents.count, 0u);
    XCTAssertEqual(terminal.pendingIntents.bytes, 0u);
    terminal.readOnly = YES;
    XCTAssertEqual([owner resize:terminal.identifier columns:22 rows:5], SFM_OK);
    XCTAssertEqual(sink.sent.count, 3u);
    terminal.sink = nil;
    XCTAssertEqual([owner close:terminal.identifier], SFM_OK);
    [done fulfill];
  }];
  XCTAssertTrue(accepted);
  [self waitForExpectations:@[done] timeout:5];
}
- (void)testPreadmissionQueueReservesReleaseSpaceAndOnlyCoalescesAdjacentIntents {
  SFIntentQueue *queue = [SFIntentQueue new];
  SFNativeIntent *write = [SFNativeIntent new]; write.kind = SFIntentWrite;
  write.data = [@"a" dataUsingEncoding:NSUTF8StringEncoding];
  for (unsigned i = 0; i < 64; ++i) XCTAssertEqual([queue append:write], SFM_OK);
  XCTAssertEqual([queue append:write], SFM_BUSY);
  SFNativeIntent *release = [SFNativeIntent new]; release.kind = SFIntentRelease;
  release.data = [@"up" dataUsingEncoding:NSUTF8StringEncoding];
  XCTAssertEqual([queue append:release], SFM_OK);
  XCTAssertEqual([queue append:release], SFM_OK);
  XCTAssertEqual(queue.count, 65u);
  XCTAssertEqual(queue.bytes, 68u);
  [queue clear];
  XCTAssertEqual([queue append:release], SFM_OK);
  release.userInitiated = 1;
  XCTAssertEqual([queue append:release], SFM_OK);
  XCTAssertEqual(queue.count, 2u); // Never merge packets that carry different Rust hints.
  XCTAssertEqual(queue.front.userInitiated, 0u);
  [queue pop]; XCTAssertEqual(queue.front.userInitiated, 1u);
  [queue clear];
  SFNativeIntent *resize = [SFNativeIntent new]; resize.kind = SFIntentResize; resize.columns = 20; resize.rows = 5;
  XCTAssertEqual([queue append:resize], SFM_OK);
  resize.columns = 30; XCTAssertEqual([queue append:resize], SFM_OK);
  XCTAssertEqual(queue.count, 1u); XCTAssertEqual(queue.front.columns, 30u);
  XCTAssertEqual([queue append:write], SFM_OK);
  resize.columns = 40; XCTAssertEqual([queue append:resize], SFM_OK);
  XCTAssertEqual(queue.count, 3u); // Do not move a resize across an intervening input.
}
- (void)testHintAndNulSurviveCopyAndMixedBatchRollback {
  SFIntentQueue *queue = [SFIntentQueue new];
  uint8_t nul = 0;
  SFNativeIntent *packet = [SFNativeIntent new]; packet.kind = SFIntentWrite; packet.userInitiated = 1;
  packet.data = [NSData dataWithBytes:&nul length:1];
  XCTAssertEqual([queue append:packet], SFM_OK);
  packet.userInitiated = 0;
  XCTAssertEqual(queue.front.userInitiated, 1u);
  XCTAssertEqual(queue.front.data.length, 1u);
  uint8_t copied = 255; [queue.front.data getBytes:&copied length:1]; XCTAssertEqual(copied, 0u);
  [queue clear];
  SFNativeIntent *release = [SFNativeIntent new]; release.kind = SFIntentRelease; release.userInitiated = 0;
  release.data = [@"up" dataUsingEncoding:NSUTF8StringEncoding];
  XCTAssertEqual([queue append:release], SFM_OK);
  SFNativeIntent *invalid = [SFNativeIntent new]; invalid.kind = SFIntentWrite; invalid.userInitiated = 2;
  invalid.data = packet.data;
  NSArray *batch = @[release, invalid];
  XCTAssertEqual([queue appendBatch:batch], SFM_INVALID);
  XCTAssertEqual(queue.count, 1u); XCTAssertEqual(queue.bytes, 2u);
  XCTAssertEqualObjects(queue.front.data, release.data); XCTAssertEqual(queue.front.userInitiated, 0u);
}
- (void)testDecimalIdentifiersAreLosslessAndStrict {
  uint64_t value = 0;
  XCTAssertTrue(SFParseId(@"18446744073709551615", &value));
  XCTAssertEqual(value, UINT64_MAX);
  XCTAssertEqualObjects(SFDecimal(value), @"18446744073709551615");
  for (NSString *invalid in @[@"0", @"01", @"-1", @"1.0", @"18446744073709551616", @" 1"])
    XCTAssertFalse(SFParseId(invalid, &value));
}
- (void)testInternalSuccessEventsDoNotReachJavaScript {
  XCTestExpectation *done = [self expectationWithDescription:@"native event filter"];
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFClientRecord *client = [SFClientRecord new];
    SFClientStore *store = [SFClientStore shared];
    NSDictionary *connection = @{@"status":@"connected", @"permission":@"approved", @"connectionId":@"1"};
    NSDictionary *(^event)(NSString *) = ^NSDictionary *(NSString *operation) {
      return @{@"operationId":operation, @"internal":@(![operation isEqual:@"3"]), @"status":@0, @"connection":connection,
        @"result":@{@"ok":@YES, @"value":@{@"events":@[]}}, @"resetTickets":@{}};
    };
    XCTAssertTrue([store shouldForward:event(@"1") client:client]);
    XCTAssertFalse([store shouldForward:event(@"2") client:client]);
    [client.explicitOperations addObject:@"3"];
    XCTAssertTrue([store shouldForward:event(@"3") client:client]);
    XCTAssertFalse([client.explicitOperations containsObject:@"3"]);
    client.pollOperation = @"4";
    NSDictionary *completion = event(@"4");
    [store completed:completion client:client];
    XCTAssertFalse([store shouldForward:completion client:client]);
    XCTAssertNil(client.pollOperation);
    NSMutableDictionary *denied = [event(@"5") mutableCopy];
    denied[@"result"] = @{@"ok":@NO, @"error":@{@"code":@"FORBIDDEN", @"message":@"denied"}};
    XCTAssertTrue([store shouldForward:denied client:client]);
    XCTAssertFalse([store shouldForward:denied client:client]);
    [done fulfill];
  }];
  XCTAssertTrue(accepted);
  [self waitForExpectations:@[done] timeout:5];
}
// UITextInput call sequences as the iOS keyboard, dictation and the emoji
// keyboard make them, through the real Rust composer and Ghostty encoder.
// The bytes land in a line editor that behaves like a shell prompt.
- (void)runKeyboard:(void (^)(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted))body {
  XCTestExpectation *done = [self expectationWithDescription:@"UIKit keyboard"];
  dispatch_async(dispatch_get_main_queue(), ^{
    uint64_t terminal = 0;
    XCTAssertEqual(sfm_terminal_create(80, 24, &terminal), SFM_OK);
    SFTextInput *input = [[SFTextInput alloc] initWithFrame:CGRectMake(0, 0, 200, 40) textContainer:nil];
    NSMutableString *line = [NSMutableString new];
    NSMutableArray<NSString *> *submitted = [NSMutableArray new];
    input.sendEvent = ^(NSDictionary *event) {
      NSData *json = [NSJSONSerialization dataWithJSONObject:event options:0 error:nil];
      NSMutableData *bytes = [NSMutableData new];
      XCTAssertEqual(sfm_terminal_input(terminal, (const uint8_t *)json.bytes, json.length, SFTestAppendBytes,
        (__bridge void *)bytes), SFM_OK, @"%@", event);
      NSString *text = [[NSString alloc] initWithData:bytes encoding:NSUTF8StringEncoding];
      [text enumerateSubstringsInRange:NSMakeRange(0, text.length) options:NSStringEnumerationByComposedCharacterSequences
        usingBlock:^(NSString *c, NSRange, NSRange, BOOL *) {
          if ([c isEqual:@"\x7f"]) {
            if (line.length) [line deleteCharactersInRange:[line rangeOfComposedCharacterSequenceAtIndex:line.length - 1]];
          } else if ([c isEqual:@"\r"]) { [submitted addObject:[line copy]]; [line setString:@""]; }
          else [line appendString:c];
        }];
    };
    body(input, line, submitted);
    XCTAssertEqual(sfm_terminal_close(terminal), SFM_OK);
    [done fulfill];
  });
  [self waitForExpectations:@[done] timeout:10];
}
static void Replace(SFTextInput *input, NSUInteger start, NSUInteger length, NSString *text) {
  // Offsets are in the editor, after its leading sentinel.
  UITextPosition *from = [input positionFromPosition:input.beginningOfDocument offset:(NSInteger)(start + 1)];
  UITextPosition *to = [input positionFromPosition:from offset:(NSInteger)length];
  [input replaceRange:[input textRangeFromPosition:from toPosition:to] withText:text];
}
- (void)testMarkedTextStaysLocalAndCancellationDoesNotCommit {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input setMarkedText:@"に" selectedRange:NSMakeRange(1, 0)];
    [input setMarkedText:@"日本" selectedRange:NSMakeRange(2, 0)];
    XCTAssertEqualObjects(line, @"");
    [input unmarkText];
    XCTAssertEqualObjects(line, @"日本");
    [input setMarkedText:@"discard" selectedRange:NSMakeRange(7, 0)];
    [input cancelComposition];
    XCTAssertEqualObjects(line, @"日本");
  }];
}
- (void)testReplacingAPreviousWordSendsBackspacesAndTheNewWord {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input insertText:@"teh "];
    Replace(input, 0, 3, @"the");
    XCTAssertEqualObjects(line, @"the ");
    [input insertText:@"git stauts --short"];
    Replace(input, 8, 6, @"status");
    XCTAssertEqualObjects(line, @"the git status --short");
    [input insertText:@"\n"];
    XCTAssertEqualObjects(submitted, @[@"the git status --short"]);
  }];
}
- (void)testDictationCommitsOnlyTheFinalPhrase {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    for (NSString *partial in @[@"echo", @"echo hell", @"echo hello world"]) {
      [input setMarkedText:partial selectedRange:NSMakeRange(partial.length, 0)];
      XCTAssertEqualObjects(line, @"");
    }
    [input unmarkText];
    XCTAssertEqualObjects(line, @"echo hello world");
  }];
}
- (void)testEmojiAndBackspaceThroughEditorAndTerminalText {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input insertText:@"ok 👍🏽"];
    XCTAssertEqualObjects(line, @"ok 👍🏽");
    [input deleteBackward];
    XCTAssertEqualObjects(line, @"ok ");
    [input setMarkedText:@"wo" selectedRange:NSMakeRange(2, 0)];
    [input deleteBackward];
    XCTAssertEqualObjects(line, @"ok ");
    [input unmarkText];
    XCTAssertEqualObjects(line, @"ok w");
    for (int i = 0; i < 4; ++i) [input deleteBackward];
    XCTAssertEqualObjects(line, @"");
    [input finishAndClear];
    [input deleteBackward];
    XCTAssertEqualObjects(line, @"");
  }];
}
- (void)testEnterInTheMiddleOfCompositionCommitsThenSubmits {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input insertText:@"echo "];
    [input setMarkedText:@"hel" selectedRange:NSMakeRange(3, 0)];
    [input unmarkText];
    [input insertText:@"\n"];
    XCTAssertEqualObjects(submitted, @[@"echo hel"]);
    [input insertText:@"pwd"];
    XCTAssertEqualObjects(line, @"pwd");
  }];
}
- (void)testStickyControlTurnsTheNextTypedLetterIntoAChord {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    __block NSMutableArray *keys = [NSMutableArray new];
    void (^original)(NSDictionary *) = input.sendEvent;
    input.sendEvent = ^(NSDictionary *event) { if ([event[@"type"] isEqual:@"key"]) [keys addObject:event[@"event"]]; original(event); };
    input.stickyModifiers = 2;
    [input insertText:@"c"];
    XCTAssertEqualObjects(keys.firstObject[@"physical"], @22);
    XCTAssertEqualObjects(keys.firstObject[@"modifiers"], @2);
    XCTAssertEqualObjects(keys.firstObject[@"text"], @"c");
    input.stickyModifiers = 0;
    [input insertText:@"ls"];
    XCTAssertEqualObjects(line, @"ls");
  }];
}
@end
