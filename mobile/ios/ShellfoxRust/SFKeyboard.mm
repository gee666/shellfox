#import "SFTextInput.h"

NSDictionary *SFHardwareKey(UIKey *key, NSString *action) {
  UIKeyModifierFlags flags = key.modifierFlags;
  unsigned modifiers = ((flags & UIKeyModifierShift) ? 1 : 0) |
    ((flags & UIKeyModifierControl) ? 2 : 0) | ((flags & UIKeyModifierAlternate) ? 4 : 0) |
    ((flags & UIKeyModifierCommand) ? 8 : 0);
  NSString *kind = nil;
  switch (key.keyCode) {
    case UIKeyboardHIDUsageKeyboardReturnOrEnter: kind = @"enter"; break;
    case UIKeyboardHIDUsageKeyboardTab: kind = @"tab"; break;
    case UIKeyboardHIDUsageKeyboardDeleteOrBackspace: kind = @"backspace"; break;
    case UIKeyboardHIDUsageKeyboardEscape: kind = @"escape"; break;
    case UIKeyboardHIDUsageKeyboardRightArrow: kind = @"arrowRight"; break;
    case UIKeyboardHIDUsageKeyboardLeftArrow: kind = @"arrowLeft"; break;
    case UIKeyboardHIDUsageKeyboardDownArrow: kind = @"arrowDown"; break;
    case UIKeyboardHIDUsageKeyboardUpArrow: kind = @"arrowUp"; break;
    case UIKeyboardHIDUsageKeyboardHome: kind = @"home"; break;
    case UIKeyboardHIDUsageKeyboardEnd: kind = @"end"; break;
    case UIKeyboardHIDUsageKeyboardPageUp: kind = @"pageUp"; break;
    case UIKeyboardHIDUsageKeyboardPageDown: kind = @"pageDown"; break;
    case UIKeyboardHIDUsageKeyboardInsert: kind = @"insert"; break;
    case UIKeyboardHIDUsageKeyboardDeleteForward: kind = @"delete"; break;
    default: break;
  }
  NSMutableDictionary *physical = kind ? [@{@"kind":kind} mutableCopy] : nil;
  if (!physical && key.keyCode >= UIKeyboardHIDUsageKeyboardF1 && key.keyCode <= UIKeyboardHIDUsageKeyboardF12)
    physical = [@{@"kind":@"function", @"value":@(key.keyCode - UIKeyboardHIDUsageKeyboardF1 + 1)} mutableCopy];
  if (!physical && key.keyCode >= UIKeyboardHIDUsageKeyboardF13 && key.keyCode <= UIKeyboardHIDUsageKeyboardF24)
    physical = [@{@"kind":@"function", @"value":@(key.keyCode - UIKeyboardHIDUsageKeyboardF13 + 13)} mutableCopy];
  if (!physical && (modifiers & (2 | 4 | 8))) {
    NSString *plain = key.charactersIgnoringModifiers.lowercaseString;
    BOOL transformed = key.characters.length && ![key.characters isEqual:key.charactersIgnoringModifiers] &&
      [key.characters rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location == NSNotFound;
    // Option dead keys and AltGr-produced text belong to UIKit's composition path.
    if ((modifiers & 4) && !(modifiers & 8) && (transformed || !key.characters.length)) return nil;
    // Cmd+V is a UIKit paste action. Ctrl+C always belongs to the terminal.
    if ((modifiers & 8) && ([plain isEqualToString:@"v"] || [plain isEqualToString:@"c"])) return nil;
    if (plain.length == 1 && [plain characterAtIndex:0] >= 0x20 && [plain characterAtIndex:0] <= 0x7E)
      physical = [@{@"kind":@"character", @"value":plain} mutableCopy];
  }
  if (!physical) return nil;
  return @{@"type":@"key", @"key":physical, @"action":action, @"modifiers":@(modifiers)};
}
NSDictionary *SFHardwareTextKey(UIKey *key, NSString *action) {
  NSString *plain = key.charactersIgnoringModifiers.lowercaseString;
  UIKeyModifierFlags flags = key.modifierFlags;
  unsigned modifiers = ((flags & UIKeyModifierShift) ? 1 : 0) | ((flags & UIKeyModifierControl) ? 2 : 0) |
    ((flags & UIKeyModifierAlternate) ? 4 : 0) | ((flags & UIKeyModifierCommand) ? 8 : 0);
  NSDictionary *physical = @{@"kind":@"unidentified"};
  if (plain.length == 1 && [plain characterAtIndex:0] >= 32 && [plain characterAtIndex:0] < 127)
    physical = @{@"kind":@"character", @"value":plain};
  unsigned consumed = modifiers & (1 | 4);
  if ((modifiers & 6) == 6 && ![key.characters isEqual:key.charactersIgnoringModifiers]) consumed |= 2;
  uint32_t unshifted = plain.length == 1 && !CFStringIsSurrogateHighCharacter([plain characterAtIndex:0]) &&
    !CFStringIsSurrogateLowCharacter([plain characterAtIndex:0]) ? [plain characterAtIndex:0] : 0;
  return @{@"type":@"key", @"key":physical, @"action":action, @"modifiers":@(modifiers),
    @"consumedModifiers":@(consumed), @"unshiftedCodepoint":@(unshifted)};
}
