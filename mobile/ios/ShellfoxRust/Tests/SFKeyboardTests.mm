#import <XCTest/XCTest.h>
#import "../SFTextInput.h"
#import "../SFOwner.h"

static void SFTestAppendBytes(void *context, const uint8_t *bytes, size_t length) {
  [(__bridge NSMutableData *)context appendBytes:bytes length:length];
}

@interface SFKeyboardTests : XCTestCase
@end

@implementation SFKeyboardTests
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
            // Like readline: one backspace erases one code point.
            NSUInteger n = line.length >= 2 && CFStringIsSurrogateLowCharacter([line characterAtIndex:line.length - 1]) ? 2 : 1;
            if (line.length) [line deleteCharactersInRange:NSMakeRange(line.length - n, n)];
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
  // Keyboards leave the cursor at the end of what the user was typing.
  input.selectedRange = NSMakeRange(input.text.length, 0);
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
- (void)testStickyModifierWithReturnIsEnterAndTabEndsTheEditor {
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input insertText:@"ls"];
    input.stickyModifiers = 2;
    [input insertText:@"\n"];
    input.stickyModifiers = 0;
    XCTAssertEqualObjects(submitted, @[@"ls"]);
    [input insertText:@"cd \t"];
    XCTAssertEqualObjects(line, @"cd \t");
    // After Tab the editor starts over: Backspace reaches the terminal.
    [input deleteBackward];
    XCTAssertEqualObjects(line, @"cd ");
  }];
}
- (void)testALongKeyboardInsertIsSentAsPastePieces {
  NSString *text = [@"" stringByPaddingToLength:40000 withString:@"x" startingAtIndex:0];
  NSString *fox = [text stringByAppendingString:@"🦊"];
  NSMutableString *joined = [NSMutableString new];
  NSArray *events = SFPasteEvents(fox);
  XCTAssertGreaterThan(events.count, 1u);
  for (NSDictionary *event in events) {
    XCTAssertEqualObjects(event[@"type"], @"paste");
    XCTAssertLessThanOrEqual([event[@"text"] length], 16u * 1024);
    [joined appendString:event[@"text"]];
  }
  XCTAssertEqualObjects(joined, fox);
  [self runKeyboard:^(SFTextInput *input, NSMutableString *line, NSMutableArray<NSString *> *submitted) {
    [input insertText:text];
    XCTAssertEqual(line.length, 40000u);
    [input insertText:@"y"];
    XCTAssertEqual(line.length, 40001u);
  }];
}
@end
