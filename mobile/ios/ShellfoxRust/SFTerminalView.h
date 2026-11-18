#pragma once
#import <UIKit/UIKit.h>
#import "SFOwner.h"
#import "SFTextInput.h"

@interface SFTerminalView : UIView {
@public
  std::shared_ptr<shellfox::Mailbox> _mailbox;
  std::shared_ptr<const shellfox::Frame> _frame;
  SFFramePainter *_painter;
  SFTextInput *_input;
  NSUInteger _binding;
  CGPoint _selectionStart;
  CGFloat _scrollRemainder;
}
@property(nonatomic, copy) NSString *terminalHandle;
@property(nonatomic) BOOL readOnly;
@property(nonatomic) BOOL focused;
@property(nonatomic) BOOL selecting;
@property(nonatomic, readonly) BOOL inputReady;
@property(nonatomic) NSInteger modifiers;
@property(nonatomic) CGFloat fontSize;
@property(nonatomic, copy) void (^stateChanged)(NSString *state);
@property(nonatomic, copy) void (^focusFeedback)(NSDictionary *result);
@property(nonatomic, copy) void (^failed)(SfmStatus status);
@property(nonatomic, copy) void (^inputReceived)(void);
@property(nonatomic, copy) void (^selectionChanged)(NSString *text);
- (void)send:(NSDictionary *)event;
- (void)withTerminal:(void (^)(SFTerminal *terminal))work;
- (void)detach;
@end

typedef NS_ENUM(NSInteger, SFFocusActionSource) { SFFocusKeyboard, SFFocusSurface, SFFocusAccessibility, SFFocusReadiness };
@interface SFTerminalView (FocusFeedback)
- (BOOL)focusInputForAction:(SFFocusActionSource)source attempt:(BOOL)attempt;
@end

@interface SFTerminalView (Touch)
- (void)installGestures;
- (void)copySelection;
- (void)notifySelection;
- (void)pasteClipboard;
- (void)selectAt:(CGPoint)point to:(CGPoint)end kind:(uint32_t)kind;
- (void)scrollRows:(int32_t)rows;
- (void)wheelOrScrollRows:(int32_t)rows at:(CGPoint)point;
- (void)openLinkAt:(CGPoint)point;
@end
