#import "SFTerminalView.h"

@implementation SFTerminalView (FocusFeedback)
- (BOOL)focusInputForAction:(SFFocusActionSource)source attempt:(BOOL)attempt {
  NSString *name;
  switch (source) {
    case SFFocusKeyboard: name = @"keyboard"; break;
    case SFFocusSurface: name = @"surface"; break;
    case SFFocusAccessibility: name = @"accessibility"; break;
    case SFFocusReadiness: name = @"readiness"; break;
    default: return NO;
  }
  // Caller supplies its existing guard. This replaces one existing request,
  // never retries or changes parser/controller readiness.
  NSUInteger binding = _binding;
  NSString *handle = self.terminalHandle;
  auto mailbox = _mailbox;
  uint64_t epoch = mailbox ? mailbox->epoch.load() : 0;
  BOOL allowed = self.inputReady, frame = bool(_frame);
  BOOL attached = self.window != nil, windowFocused = self.window.isKeyWindow;
  BOOL uiThread = NSThread.isMainThread;
  BOOL accepted = attempt ? [_input becomeFirstResponder] : NO;
  NSDictionary *result = @{
    @"platform":@"ios", @"source":name, @"attached":@(attached),
    @"windowFocused":@(windowFocused), @"uiThread":@(uiThread), @"inputAllowed":@(allowed),
    @"frameAvailable":@(frame), @"focusRequest":attempt ? (accepted ? @"accepted" : @"rejected") : @"notAttempted",
    @"focused":@(_input.isFirstResponder), @"keyboardRequest":@"notApplicable"
  };
  __weak SFTerminalView *weakSelf = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    SFTerminalView *view = weakSelf;
    if (!view || view->_binding != binding || ![view.terminalHandle isEqual:handle] ||
      view->_mailbox != mailbox || (mailbox && mailbox->epoch.load() != epoch)) return;
    if (view.focusFeedback) view.focusFeedback(result);
  });
  // First-responder acceptance is not a keyboard-visibility receipt.
  return accepted;
}
@end
