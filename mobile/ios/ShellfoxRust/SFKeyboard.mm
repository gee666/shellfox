#import "SFTextInput.h"

/// HID usage to the public GhosttyKey number. Unknown keys stay UIKit's.
static NSNumber *Physical(NSUInteger hid) {
  if (hid >= 4 && hid <= 29) return @(20 + hid - 4);
  if (hid >= 30 && hid <= 38) return @(7 + hid - 30);
  if (hid >= 58 && hid <= 69) return @(121 + hid - 58);
  if (hid >= 104 && hid <= 115) return @(133 + hid - 104);
  if (hid >= 89 && hid <= 97) return @(81 + hid - 89);
  static NSDictionary<NSNumber *, NSNumber *> *named;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ named = @{
    @39:@6, @40:@58, @41:@120, @42:@53, @43:@64, @44:@63,
    @45:@46, @46:@16, @47:@3, @48:@4, @49:@2, @51:@49,
    @52:@48, @53:@1, @54:@5, @55:@47, @56:@50, @57:@54,
    @70:@148, @71:@149, @72:@150, @73:@72, @74:@71, @75:@74,
    @76:@68, @77:@69, @78:@73, @79:@77, @80:@76, @81:@75, @82:@78,
    @83:@79, @84:@96, @85:@104, @86:@107, @87:@90, @88:@97,
    @98:@80, @99:@95, @100:@17, @101:@55, @103:@98, @133:@94,
    @135:@18, @137:@19,
    @224:@56, @225:@61, @226:@51, @227:@59,
    @228:@57, @229:@62, @230:@52, @231:@60
  }; });
  return named[@(hid)];
}

static BOOL Printable(NSString *text) {
  if (!text.length) return NO;
  __block BOOL ok = YES;
  [text enumerateSubstringsInRange:NSMakeRange(0, text.length) options:NSStringEnumerationByComposedCharacterSequences
    usingBlock:^(NSString *s, NSRange, NSRange, BOOL *stop) {
      unichar c = [s characterAtIndex:0];
      if (c < 0x20 || c == 0x7f || (c >= 0xf700 && c <= 0xf8ff)) { ok = NO; *stop = YES; }
    }];
  return ok;
}

NSDictionary *SFKey(NSNumber *physical, NSString *action, NSUInteger modifiers, NSString *text, id unshifted) {
  return @{@"type":@"key", @"event":@{@"physical":physical, @"action":action, @"modifiers":@(modifiers),
    @"consumedModifiers":@0, @"text":text ?: @"", @"unshiftedCodepoint":unshifted ?: NSNull.null, @"composing":@NO}};
}

NSDictionary *SFHardwareKey(UIKey *key, NSString *action) {
  NSNumber *physical = Physical(key.keyCode);
  if (!physical) return nil;
  UIKeyModifierFlags flags = key.modifierFlags;
  NSUInteger modifiers = ((flags & UIKeyModifierShift) ? 1 : 0) | ((flags & UIKeyModifierControl) ? 2 : 0) |
    ((flags & UIKeyModifierAlternate) ? 4 : 0) | ((flags & UIKeyModifierCommand) ? 8 : 0) |
    ((flags & UIKeyModifierAlphaShift) ? 16 : 0);
  NSString *plain = key.charactersIgnoringModifiers;
  BOOL printable = Printable(plain) && [plain rangeOfCharacterFromSet:NSCharacterSet.newlineCharacterSet].location == NSNotFound;
  // Plain and Shift text, Option-composed text and dead keys go through UIKit
  // and the keyboard editor, exactly once.
  if (printable && !(modifiers & (2 | 8))) return nil;
  // Cmd+C/V are UIKit copy and paste; Ctrl+C always belongs to the terminal.
  NSString *lower = plain.lowercaseString;
  if ((modifiers & 8) && ([lower isEqual:@"c"] || [lower isEqual:@"v"])) return nil;
  id unshifted = nil;
  if (lower.length == 1 && !CFStringIsSurrogateHighCharacter([lower characterAtIndex:0]) &&
    !CFStringIsSurrogateLowCharacter([lower characterAtIndex:0]) && printable) unshifted = @([lower characterAtIndex:0]);
  NSString *text = [action isEqual:@"release"] || !printable ? @"" : plain;
  return SFKey(physical, action, modifiers, text, unshifted);
}

NSDictionary *SFReleasedKey(NSDictionary *pressed) {
  NSMutableDictionary *event = [pressed[@"event"] mutableCopy];
  event[@"action"] = @"release"; event[@"text"] = @"";
  return @{@"type":@"key", @"event":event};
}

/// The US key that types this ASCII character unshifted, or Unidentified.
NSNumber *SFUSPosition(unichar c) {
  if (c >= 'a' && c <= 'z') return @(20 + c - 'a');
  if (c >= '0' && c <= '9') return @(6 + c - '0');
  NSDictionary *named = @{@"`":@1, @"\\":@2, @"[":@3, @"]":@4, @",":@5, @"=":@16, @"-":@46, @".":@47,
    @"'":@48, @";":@49, @"/":@50, @" ":@63};
  return named[[NSString stringWithCharacters:&c length:1]] ?: @0;
}
