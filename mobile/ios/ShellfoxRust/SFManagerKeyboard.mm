#import "SFTextInput.h"

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
    @98:@80, @99:@95, @100:@17, @101:@55, @102:@167, @103:@98,
    @127:@170, @128:@171, @129:@169, @133:@94,
    @135:@18, @136:@66, @137:@19, @138:@65, @139:@67,
    @224:@56, @225:@61, @226:@51, @227:@59,
    @228:@57, @229:@62, @230:@52, @231:@60
  }; });
  return named[@(hid)];
}
NSDictionary *SFManagerLogicalKey(NSString *text, NSUInteger modifiers) {
  return @{@"kind":@"key", @"event":@{@"physical":@0, @"action":@"press",
    @"modifiers":@(modifiers), @"consumedModifiers":@0, @"text":text,
    @"unshiftedCodepoint":NSNull.null, @"composing":@NO}};
}
NSDictionary *SFManagerHardwareKey(UIKey *key, NSString *action, NSString *text, NSUInteger sides) {
  NSNumber *physical = Physical(key.keyCode);
  if (!physical) return nil; // Unknown HID usage is not a logical-key fallback.
  UIKeyModifierFlags flags = key.modifierFlags;
  NSUInteger modifiers = sides | ((flags & UIKeyModifierShift) ? 1 : 0) |
    ((flags & UIKeyModifierControl) ? 2 : 0) | ((flags & UIKeyModifierAlternate) ? 4 : 0) |
    ((flags & UIKeyModifierCommand) ? 8 : 0) | ((flags & UIKeyModifierAlphaShift) ? 16 : 0);
  NSString *plain = key.charactersIgnoringModifiers.lowercaseString;
  id scalar = NSNull.null;
  if (plain.length == 1 && !CFStringIsSurrogateHighCharacter([plain characterAtIndex:0]) &&
    !CFStringIsSurrogateLowCharacter([plain characterAtIndex:0])) scalar = @([plain characterAtIndex:0]);
  if ([action isEqual:@"release"] || [text rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound) text = @"";
  // UIKit supplies no consumed-modifier mask. Preserve zero rather than guessing.
  return @{@"kind":@"key", @"event":@{@"physical":physical, @"action":action,
    @"modifiers":@(modifiers), @"consumedModifiers":@0, @"text":text ?: @"",
    @"unshiftedCodepoint":scalar, @"composing":@NO}};
}
NSDictionary *SFReleasedKey(NSDictionary *pressed) {
  NSMutableDictionary *released = [pressed mutableCopy];
  if ([pressed[@"kind"] isEqual:@"key"]) {
    NSMutableDictionary *event = [pressed[@"event"] mutableCopy];
    event[@"action"] = @"release"; event[@"text"] = @""; released[@"event"] = event;
  } else { released[@"action"] = @"release"; [released removeObjectForKey:@"text"]; }
  return released;
}
