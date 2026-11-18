#import "SFTextInput.h"

static NSString *const Sentinel = @"\u200B";
@implementation SFTextInput {
  BOOL _resetting;
  NSMutableDictionary<NSNumber *, NSDictionary *> *_physical;
  NSMutableSet<NSNumber *> *_textPhysical;
  UIKey *_sourceKey;
  NSString *_sourceAction;
  NSUInteger _managerSides;
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
    self.autocorrectionType = UITextAutocorrectionTypeNo;
    self.autocapitalizationType = UITextAutocapitalizationTypeNone;
    self.smartQuotesType = UITextSmartQuotesTypeNo;
    self.smartDashesType = UITextSmartDashesTypeNo;
    self.smartInsertDeleteType = UITextSmartInsertDeleteTypeNo;
    self.spellCheckingType = UITextSpellCheckingTypeNo;
    self.scrollEnabled = NO;
    self.accessibilityLabel = @"Terminal keyboard";
    _physical = [NSMutableDictionary new]; _textPhysical = [NSMutableSet new];
  }
  return self;
}
- (void)resetDocument {
  _resetting = YES;
  self.text = Sentinel;
  self.selectedRange = NSMakeRange(Sentinel.length, 0);
  _resetting = NO;
}
- (void)textViewDidChange:(UITextView *)textView {
  if (_resetting) return;
  NSString *text = [self.text hasPrefix:Sentinel] ? [self.text substringFromIndex:Sentinel.length] : self.text;
  if ([text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > 60000) {
    [self cancelComposition]; return;
  }
  if (self.markedTextRange) {
    _sourceKey = nil;
    if (self.sendEvent) self.sendEvent(@{@"type":@"preedit", @"text":text});
    return;
  }
  UIKey *source = _sourceKey; NSString *action = _sourceAction ?: @"press"; _sourceKey = nil;
  [self resetDocument];
  if (!text.length || !self.sendEvent) return;
  text = [text stringByReplacingOccurrencesOfString:@"\r\n" withString:@"\n"];
  if (source && [text rangeOfCharacterFromSet:NSCharacterSet.newlineCharacterSet].location == NSNotFound) {
    NSMutableDictionary *event = [self.hostEncoding ? SFManagerHardwareKey(source, action, text, _managerSides) : SFHardwareTextKey(source, action) mutableCopy];
    if (!event) return;
    if (!self.hostEncoding) event[@"text"] = text;
    self.sendEvent(event);
    _physical[@(source.keyCode)] = event; [_textPhysical addObject:@(source.keyCode)];
    return;
  }
  // UIKit can commit a newline as part of software-keyboard input.
  NSArray<NSString *> *lines = [text componentsSeparatedByCharactersInSet:NSCharacterSet.newlineCharacterSet];
  for (NSUInteger i = 0; i < lines.count; ++i) {
    if (i) self.sendEvent(@{@"type":@"key", @"key":@{@"kind":@"enter"}, @"action":@"press"});
    if (lines[i].length) self.sendEvent(@{@"type":@"commit", @"text":lines[i]});
  }
}
- (void)unmarkText {
  [super unmarkText];
  [self textViewDidChange:self];
}
- (void)deleteBackward {
  if (!self.markedTextRange && [self.text isEqualToString:Sentinel]) {
    if (self.sendEvent) self.sendEvent(@{@"type":@"key", @"key":@{@"kind":@"backspace"}, @"action":@"press"});
  } else {
    [super deleteBackward];
  }
}
- (void)cancelComposition {
  // Discard marked text on detach/background, never commit into a different tab.
  _resetting = YES;
  [super unmarkText];
  _resetting = NO;
  [self resetDocument];
  for (NSDictionary *pressed in _physical.allValues) {
    if (self.sendEvent) self.sendEvent(SFReleasedKey(pressed));
  }
  [_physical removeAllObjects]; [_textPhysical removeAllObjects]; _sourceKey = nil; _sourceAction = nil; _managerSides = 0;
}
- (void)textViewDidEndEditing:(UITextView *)textView { [self cancelComposition]; }
- (void)paste:(id)sender { if (self.requestPaste) self.requestPaste(); }
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
    NSDictionary *encoded = key ? SFHardwareKey(key, _physical[@(key.keyCode)] != nil ? @"repeat" : @"press") : nil;
    if (self.hostEncoding && key) {
      NSUInteger side = key.keyCode == 228 ? 128 : key.keyCode == 229 ? 64 : key.keyCode == 230 ? 256 : key.keyCode == 231 ? 512 : 0;
      _managerSides |= side;
      if (encoded || (key.keyCode >= 224 && key.keyCode <= 231))
        encoded = SFManagerHardwareKey(key, _physical[@(key.keyCode)] ? @"repeat" : @"press", @"", _managerSides);
    }
    if (encoded && !self.markedTextRange) {
      _physical[@(key.keyCode)] = encoded;
      if (self.sendEvent) self.sendEvent(encoded);
    } else [ordinary addObject:press];
  }
  if (ordinary.count == 1) {
    UIKey *key = ((UIPress *)ordinary.anyObject).key;
    _sourceKey = key; _sourceAction = _physical[@(key.keyCode)] ? @"repeat" : @"press";
  } else _sourceKey = nil;
  if (ordinary.count) [super pressesBegan:ordinary withEvent:event];
}
- (void)endPresses:(NSSet<UIPress *> *)presses event:(UIPressesEvent *)event cancelled:(BOOL)cancelled {
  NSMutableSet *ordinary = [NSMutableSet new];
  for (UIPress *press in presses) {
    if (press.key && _physical[@(press.key.keyCode)]) {
      NSDictionary *encoded = SFReleasedKey(_physical[@(press.key.keyCode)]);
      [_physical removeObjectForKey:@(press.key.keyCode)];
      if (self.sendEvent) self.sendEvent(encoded);
      if ([_textPhysical containsObject:@(press.key.keyCode)]) {
        [_textPhysical removeObject:@(press.key.keyCode)]; [ordinary addObject:press];
      }
    } else [ordinary addObject:press];
    if (press.key) {
      NSUInteger side = press.key.keyCode == 228 ? 128 : press.key.keyCode == 229 ? 64 : press.key.keyCode == 230 ? 256 : press.key.keyCode == 231 ? 512 : 0;
      _managerSides &= ~side;
      if (_sourceKey.keyCode == press.key.keyCode) _sourceKey = nil;
    }
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
