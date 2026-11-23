#pragma once
#import <UIKit/UIKit.h>

/// The keyboard's own small editor: the text typed since the last Enter or
/// key. Its committed text goes to Rust after every edit; marked text stays.
@interface SFTextInput : UITextView <UITextViewDelegate>
@property(nonatomic, copy) void (^sendEvent)(NSDictionary *event);
@property(nonatomic, copy) void (^requestPaste)(void);
@property(nonatomic, copy) void (^requestCopy)(void);
/// Key-bar modifiers (shift 1, ctrl 2, alt 4, cmd 8) for the next typed key.
@property(nonatomic) NSUInteger stickyModifiers;
- (void)cancelComposition;
/// Keeps what was typed and starts the next text in an empty editor.
- (void)finishAndClear;
@end

// Returns nil for ordinary text, dead keys and IME. UIKit commits those once.
NSDictionary *SFHardwareKey(UIKey *key, NSString *action);
NSDictionary *SFKey(NSNumber *physical, NSString *action, NSUInteger modifiers, NSString *text, id unshifted);
NSDictionary *SFReleasedKey(NSDictionary *pressed);
NSNumber *SFUSPosition(unichar c);
/// A paste as input events that each fit one write.
NSArray<NSDictionary *> *SFPasteEvents(NSString *text);
