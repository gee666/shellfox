#import "SFTextInput.h"

// A leading zero-width space lets Backspace on an empty editor reach
// deleteBackward, which then deletes terminal text.
static NSString *const Sentinel = @"\u200B";
static const NSUInteger Backspace = 53, Enter = 58, Tab = 64;

@implementation SFTextInput {
  BOOL _resetting;
  NSString *_reported;
  NSMutableDictionary<NSNumber *, NSDictionary *> *_physical;
  NSMutableDictionary<NSNumber *, NSTimer *> *_repeats;
}
- (instancetype)initWithFrame:(CGRect)frame textContainer:(NSTextContainer *)container {
  if ((self = [super initWithFrame:frame textContainer:container])) {
    self.delegate = self;
    self.text = Sentinel;
    self.selectedRange = NSMakeRange(Sentinel.length, 0);
    self.backgroundColor = UIColor.clearColor;
    self.textColor = UIColor.clearColor;
    self.tintColor = UIColor.clearColor;
    self.textContainerInset = UIEdgeInsetsZero;
    self.textContainer.lineFragmentPadding = 0;
    self.markedTextStyle = @{NSForegroundColorAttributeName:UIColor.whiteColor,
      NSBackgroundColorAttributeName:UIColor.blackColor, NSUnderlineStyleAttributeName:@1};
    // As 0.9.15's WebView textarea: no autocorrect, capitals or smart punctuation.
    self.autocorrectionType = UITextAutocorrectionTypeNo;
    self.autocapitalizationType = UITextAutocapitalizationTypeNone;
    self.smartQuotesType = UITextSmartQuotesTypeNo;
    self.smartDashesType = UITextSmartDashesTypeNo;
    self.smartInsertDeleteType = UITextSmartInsertDeleteTypeNo;
    self.spellCheckingType = UITextSpellCheckingTypeNo;
    self.scrollEnabled = NO;
    self.accessibilityLabel = @"Terminal keyboard";
    _reported = @"";
    _physical = [NSMutableDictionary new];
    _repeats = [NSMutableDictionary new];
  }
  return self;
}
- (void)send:(NSDictionary *)event { if (self.sendEvent) self.sendEvent(event); }
- (void)tap:(NSUInteger)physical modifiers:(NSUInteger)modifiers text:(NSString *)text unshifted:(id)unshifted {
  NSDictionary *press = SFKey(@(physical), @"press", modifiers, text, unshifted);
  [self send:press]; [self send:SFReleasedKey(press)];
}
- (NSString *)editorText {
  return [self.text hasPrefix:Sentinel] ? [self.text substringFromIndex:Sentinel.length] : self.text;
}
- (void)clearEditor {
  _resetting = YES;
  [super unmarkText];
  self.text = Sentinel;
  self.selectedRange = NSMakeRange(Sentinel.length, 0);
  _resetting = NO;
  [self send:@{@"type":@"editorReset"}];
  _reported = @"";
}
- (void)textViewDidChange:(UITextView *)textView {
  if (_resetting) return;
  NSString *full = self.editorText;
  NSString *committed = full, *marked = @"";
  UITextRange *range = self.markedTextRange;
  if (range) {
    NSInteger start = [self offsetFromPosition:self.beginningOfDocument toPosition:range.start] - (NSInteger)Sentinel.length;
    NSInteger end = [self offsetFromPosition:self.beginningOfDocument toPosition:range.end] - (NSInteger)Sentinel.length;
    if (start >= 0 && end >= start && end <= (NSInteger)full.length) {
      marked = [full substringWithRange:NSMakeRange(start, end - start)];
      committed = [full stringByReplacingCharactersInRange:NSMakeRange(start, end - start) withString:@""];
    }
  }
  // Each edit is sent whole; an editor past one write's worth starts over.
  if ([full lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > 60000) { [self clearEditor]; return; }
  // -hasPrefix: is NO for an empty prefix, so an empty editor is checked first.
  NSString *typed = !_reported.length ? full
    : [full hasPrefix:_reported] ? [full substringFromIndex:_reported.length] : nil;
  if (self.stickyModifiers && typed.length) {
    // A held key-bar modifier applies to the characters just typed.
    NSUInteger modifiers = self.stickyModifiers;
    [typed enumerateSubstringsInRange:NSMakeRange(0, typed.length) options:NSStringEnumerationByComposedCharacterSequences
      usingBlock:^(NSString *c, NSRange, NSRange, BOOL *) {
        if ([c rangeOfCharacterFromSet:NSCharacterSet.newlineCharacterSet].location != NSNotFound) {
          // As in 0.9: Ctrl+Return is Return, Alt+Return is ESC Return.
          [self tap:Enter modifiers:modifiers & 4 text:@"" unshifted:nil]; return;
        }
        if ([c isEqualToString:@"\t"]) { [self tap:Tab modifiers:modifiers & 1 text:@"" unshifted:nil]; return; }
        NSString *base = c.lowercaseString;
        unichar first = [base characterAtIndex:0];
        id unshifted = base.length == 1 ? @(first) : nil;
        [self tap:(base.length == 1 ? SFUSPosition(first).unsignedIntegerValue : 0) modifiers:modifiers text:c unshifted:unshifted];
      }];
    [self clearEditor];
    return;
  }
  if (![committed isEqualToString:_reported]) [self send:@{@"type":@"editor", @"committed":committed}];
  _reported = committed;
  // Return and Tab change the line; the next text starts in an empty editor.
  if ([committed rangeOfCharacterFromSet:NSCharacterSet.newlineCharacterSet].location != NSNotFound ||
      [committed containsString:@"\t"]) [self clearEditor];
}
- (void)insertText:(NSString *)text {
  // A keyboard's clipboard paste of a long text is a paste, not typing.
  if (text.length > 1024) {
    [self finishAndClear];
    for (NSDictionary *paste in SFPasteEvents(text)) [self send:paste];
    return;
  }
  [super insertText:text];
  [self textViewDidChange:self];
}
// UIKit tells the delegate only about some edits (none while the view is not
// first responder). Reporting after each keyboard call is idempotent.
- (void)replaceRange:(UITextRange *)range withText:(NSString *)text {
  [super replaceRange:range withText:text];
  [self textViewDidChange:self];
}
- (void)setMarkedText:(NSString *)markedText selectedRange:(NSRange)selectedRange {
  [super setMarkedText:markedText selectedRange:selectedRange];
  [self textViewDidChange:self];
}
- (void)unmarkText {
  [super unmarkText];
  [self textViewDidChange:self];
}
- (void)finishAndClear {
  if (!_reported.length && [self.text isEqualToString:Sentinel] && !self.markedTextRange) return;
  _resetting = YES; [super unmarkText]; _resetting = NO;
  [self textViewDidChange:self];
  [self clearEditor];
}
- (void)deleteBackward {
  if (!self.markedTextRange && [self.text isEqualToString:Sentinel]) {
    [self tap:Backspace modifiers:0 text:@"" unshifted:nil];
  } else {
    [super deleteBackward];
    [self textViewDidChange:self];
  }
}
- (void)cancelComposition {
  // Discard marked text on detach/background, never commit into a different tab.
  [self clearEditor];
  for (NSTimer *timer in _repeats.allValues) [timer invalidate];
  [_repeats removeAllObjects];
  for (NSDictionary *pressed in _physical.allValues) [self send:SFReleasedKey(pressed)];
  [_physical removeAllObjects];
}
// UIKit reports a held hardware key once; repeat it like the system does.
- (void)repeatKey:(NSNumber *)code {
  NSDictionary *pressed = _physical[code];
  if (!pressed) return;
  NSMutableDictionary *event = [pressed[@"event"] mutableCopy];
  event[@"action"] = @"repeat";
  NSDictionary *repeat = @{@"type":@"key", @"event":event};
  __weak SFTextInput *weakSelf = self;
  _repeats[code] = [NSTimer scheduledTimerWithTimeInterval:0.5 repeats:NO block:^(NSTimer *) {
    SFTextInput *input = weakSelf;
    if (!input || !input->_physical[code]) return;
    input->_repeats[code] = [NSTimer scheduledTimerWithTimeInterval:1.0 / 30 repeats:YES block:^(NSTimer *timer) {
      SFTextInput *held = weakSelf;
      if (!held || !held->_physical[code]) { [timer invalidate]; return; }
      [held send:repeat];
    }];
  }];
}
- (void)textViewDidEndEditing:(UITextView *)textView { [self cancelComposition]; }
- (void)paste:(id)sender { [self finishAndClear]; if (self.requestPaste) self.requestPaste(); }
- (void)copy:(id)sender { if (self.requestCopy) self.requestCopy(); }
- (NSArray<UIKeyCommand *> *)keyCommands {
  return @[[UIKeyCommand keyCommandWithInput:@"c" modifierFlags:UIKeyModifierCommand action:@selector(copy:)],
    [UIKeyCommand keyCommandWithInput:@"v" modifierFlags:UIKeyModifierCommand action:@selector(paste:)]];
}
- (BOOL)canPerformAction:(SEL)action withSender:(id)sender {
  return action == @selector(copy:) || (action == @selector(paste:) && self.editable);
}
- (BOOL)pointInside:(CGPoint)point withEvent:(UIEvent *)event { return NO; }
- (CGRect)caretRectForPosition:(UITextPosition *)position { return CGRectZero; }
- (NSArray<UITextSelectionRect *> *)selectionRectsForRange:(UITextRange *)range { return @[]; }
- (void)pressesBegan:(NSSet<UIPress *> *)presses withEvent:(UIPressesEvent *)event {
  NSMutableSet *ordinary = [NSMutableSet new];
  for (UIPress *press in presses) {
    UIKey *key = press.key;
    NSDictionary *encoded = key && !self.markedTextRange
      ? SFHardwareKey(key, _physical[@(key.keyCode)] ? @"repeat" : @"press") : nil;
    if (!encoded) { [ordinary addObject:press]; continue; }
    // A key reaches the terminal outside the editor: keep what was typed,
    // and start the next text fresh.
    [self finishAndClear];
    if (self.stickyModifiers) {
      NSMutableDictionary *withSticky = [encoded mutableCopy];
      NSMutableDictionary *inner = [encoded[@"event"] mutableCopy];
      inner[@"modifiers"] = @([inner[@"modifiers"] unsignedIntegerValue] | self.stickyModifiers);
      withSticky[@"event"] = inner; encoded = withSticky;
    }
    _physical[@(key.keyCode)] = encoded;
    [self send:encoded];
    // Modifier keys (HID 224-231) do not repeat.
    if (key.keyCode < 224 || key.keyCode > 231) [self repeatKey:@(key.keyCode)];
  }
  if (ordinary.count) [super pressesBegan:ordinary withEvent:event];
}
- (void)endPresses:(NSSet<UIPress *> *)presses event:(UIPressesEvent *)event cancelled:(BOOL)cancelled {
  NSMutableSet *ordinary = [NSMutableSet new];
  for (UIPress *press in presses) {
    NSDictionary *pressed = press.key ? _physical[@(press.key.keyCode)] : nil;
    if (pressed) {
      [_repeats[@(press.key.keyCode)] invalidate];
      [_repeats removeObjectForKey:@(press.key.keyCode)];
      [_physical removeObjectForKey:@(press.key.keyCode)];
      [self send:SFReleasedKey(pressed)];
    } else [ordinary addObject:press];
  }
  if (ordinary.count) {
    if (cancelled) [super pressesCancelled:ordinary withEvent:event];
    else [super pressesEnded:ordinary withEvent:event];
  }
}
- (void)pressesEnded:(NSSet<UIPress *> *)presses withEvent:(UIPressesEvent *)event {
  [self endPresses:presses event:event cancelled:NO];
}
- (void)pressesCancelled:(NSSet<UIPress *> *)presses withEvent:(UIPressesEvent *)event {
  [self endPresses:presses event:event cancelled:YES];
}
@end
