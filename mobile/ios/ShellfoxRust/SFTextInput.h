#pragma once
#import <UIKit/UIKit.h>

@interface SFTextInput : UITextView <UITextViewDelegate>
@property(nonatomic, copy) void (^sendEvent)(NSDictionary *event);
@property(nonatomic, copy) void (^requestPaste)(void);
@property(nonatomic, copy) void (^requestCopy)(void);
- (void)cancelComposition;
@end

// Returns nil for ordinary text, dead keys and IME. UIKit commits those once.
NSDictionary *SFHardwareKey(UIKey *key, NSString *action);
NSDictionary *SFHardwareTextKey(UIKey *key, NSString *action);
NSDictionary *SFReleasedKey(NSDictionary *pressed);
