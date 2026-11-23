#import "SFTerminalView.h"
#include <algorithm>
#include <cmath>
#include <cstdlib>

struct SFTextOutput { NSMutableData *data; bool failed = false; };
static void CopyText(void *context, const uint8_t *bytes, size_t length) {
  auto &output = *static_cast<SFTextOutput *>(context);
  if (length > 1048576 || output.data.length > 1048576 - length) { output.failed = true; return; }
  @try { [output.data appendBytes:bytes length:length]; }
  @catch (NSException *exception) { output.failed = true; }
}
static UIViewController *Presenter(UIView *view) {
  UIResponder *next = view;
  while ((next = next.nextResponder)) if ([next isKindOfClass:UIViewController.class]) return (UIViewController *)next;
  return nil;
}
static CGPoint CellPoint(SFTerminalView *view, CGPoint point) {
  if (!view->_frame) return CGPointZero;
  CGSize cell = view->_painter.cellSize;
  return CGPointMake(std::clamp(floor(point.x / cell.width), 0.0, double(view->_frame->cols - 1)),
    std::clamp(floor(point.y / cell.height), 0.0, double(view->_frame->rows - 1)));
}

@implementation SFTerminalView (Touch)
- (void)installGestures {
  UITapGestureRecognizer *tap = [[UITapGestureRecognizer alloc] initWithTarget:self action:@selector(tap:)];
  [self addGestureRecognizer:tap];
  UILongPressGestureRecognizer *select = [[UILongPressGestureRecognizer alloc] initWithTarget:self action:@selector(selectGesture:)];
  [self addGestureRecognizer:select];
  UIPanGestureRecognizer *scroll = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(pan:)];
  scroll.allowedScrollTypesMask = UIScrollTypeMaskAll;
  [scroll requireGestureRecognizerToFail:select];
  [self addGestureRecognizer:scroll];
}
- (void)tap:(UITapGestureRecognizer *)gesture {
  CGPoint point = [gesture locationInView:self];
  if (!self.selecting) [self fileLinkAt:point];
  [self selectAt:CGPointZero to:CGPointZero kind:0];
  if (!self.inputReady) { [self focusInputForAction:SFFocusSurface attempt:NO]; return; }
  [self focusInputForAction:SFFocusSurface attempt:YES];
  CGSize cell = _painter.cellSize;
  NSUInteger modifiers = self.modifiers, binding = _binding;
  __weak SFTerminalView *weakSelf = self;
  [self withTerminal:^(SFTerminal *terminal) {
    uint8_t tracking = 0;
    if (sfm_terminal_mouse_tracking(terminal.handle, &tracking) != SFM_OK || !tracking) return;
    unsigned width = MAX(1, (unsigned)ceil(cell.width)), height = MAX(1, (unsigned)ceil(cell.height));
    double x = std::clamp(point.x / cell.width, 0.0, terminal.columns - 0.001) * width;
    double y = std::clamp(point.y / cell.height, 0.0, terminal.rows - 0.001) * height;
    NSMutableArray *events = [NSMutableArray new];
    for (NSString *action in @[@"press", @"release"]) [events addObject:@{@"type":@"mouse", @"action":action,
      @"button":@"left", @"modifiers":@(modifiers), @"x":@(x), @"y":@(y),
      @"cellWidth":@(width), @"cellHeight":@(height), @"anyButtonPressed":@([action isEqualToString:@"press"])}];
    SfmStatus status = [[SFOwner shared] input:terminal.identifier events:events];
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (!view || view->_binding != binding) return;
      if (status != SFM_OK) { if (view.failed) view.failed(status); }
      else if (view.inputReceived) view.inputReceived();
    });
  }];
}
- (void)pan:(UIPanGestureRecognizer *)gesture {
  if (self.selecting) {
    CGPoint point = [gesture locationInView:self];
    if (gesture.state == UIGestureRecognizerStateBegan) _selectionStart = point;
    [self selectAt:_selectionStart to:point kind:1];
    if (gesture.state == UIGestureRecognizerStateEnded) [self notifySelection];
    return;
  }
  CGFloat delta = [gesture translationInView:self].y;
  [gesture setTranslation:CGPointZero inView:self];
  _scrollRemainder -= delta / _painter.cellSize.height;
  int32_t rows = (int32_t)std::clamp(trunc(_scrollRemainder), -128.0, 128.0);
  _scrollRemainder -= rows;
  if (rows) [self wheelOrScrollRows:rows at:[gesture locationInView:self]];
  if (gesture.state == UIGestureRecognizerStateEnded || gesture.state == UIGestureRecognizerStateCancelled)
    _scrollRemainder = 0;
}
- (void)wheelOrScrollRows:(int32_t)rows at:(CGPoint)point {
  CGSize cell = _painter.cellSize;
  NSUInteger modifiers = self.modifiers, binding = _binding;
  __weak SFTerminalView *weakSelf = self;
  [self withTerminal:^(SFTerminal *terminal) {
    uint8_t tracking = 0;
    SfmStatus status = sfm_terminal_mouse_tracking(terminal.handle, &tracking);
    if (status != SFM_OK) { [terminal.sink terminalFailed:terminal.identifier status:status]; return; }
    if (!tracking || terminal.readOnly || (modifiers & 1)) {
      status = sfm_terminal_scroll(terminal.handle, rows);
      if (status == SFM_OK) terminal.dirty = YES;
      else [terminal.sink terminalFailed:terminal.identifier status:status];
      return;
    }
    unsigned width = MAX(1, (unsigned)ceil(cell.width)), height = MAX(1, (unsigned)ceil(cell.height));
    double x = std::clamp(point.x / cell.width, 0.0, terminal.columns - 0.001) * width;
    double y = std::clamp(point.y / cell.height, 0.0, terminal.rows - 0.001) * height;
    NSMutableArray *events = [NSMutableArray new];
    for (unsigned i = 0; i < std::min(unsigned(std::abs(rows)), 128u); ++i)
      [events addObject:@{@"type":@"mouse", @"action":@"press", @"button":rows < 0 ? @"wheelUp" : @"wheelDown",
        @"modifiers":@(modifiers), @"x":@(x), @"y":@(y), @"cellWidth":@(width), @"cellHeight":@(height), @"anyButtonPressed":@NO}];
    status = [[SFOwner shared] input:terminal.identifier events:events];
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (!view || view->_binding != binding) return;
      if (status != SFM_OK) { if (view.failed) view.failed(status); }
      else if (view.inputReceived) view.inputReceived();
    });
  }];
}
- (void)scrollRows:(int32_t)rows {
  [self withTerminal:^(SFTerminal *terminal) {
    SfmStatus status = sfm_terminal_scroll(terminal.handle, rows);
    if (status == SFM_OK) terminal.dirty = YES;
    else [terminal.sink terminalFailed:terminal.identifier status:status];
  }];
}
- (void)selectAt:(CGPoint)point to:(CGPoint)end kind:(uint32_t)kind {
  if (!_frame) return;
  CGPoint startCell = CellPoint(self, point), endCell = CellPoint(self, end);
  SfmSelection selection = {kind, (uint16_t)startCell.x, (uint16_t)startCell.y,
    (uint16_t)endCell.x, (uint16_t)endCell.y, 0};
  [self withTerminal:^(SFTerminal *terminal) {
    SfmStatus status = sfm_terminal_select(terminal.handle, selection);
    if (status == SFM_OK) terminal.dirty = YES;
    else [terminal.sink terminalFailed:terminal.identifier status:status];
  }];
}
- (void)selectGesture:(UILongPressGestureRecognizer *)gesture {
  CGPoint point = [gesture locationInView:self];
  if (gesture.state == UIGestureRecognizerStateBegan) {
    _selectionStart = point; [self selectAt:point to:point kind:2];
  } else if (gesture.state == UIGestureRecognizerStateChanged) {
    [self selectAt:_selectionStart to:point kind:1];
  } else if (gesture.state == UIGestureRecognizerStateEnded) {
    [self notifySelection];
    UIAlertController *menu = [UIAlertController alertControllerWithTitle:nil message:nil preferredStyle:UIAlertControllerStyleActionSheet];
    __weak SFTerminalView *weakSelf = self;
    [menu addAction:[UIAlertAction actionWithTitle:@"Copy" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) { [weakSelf copySelection]; }]];
    [menu addAction:[UIAlertAction actionWithTitle:@"Select all" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) {
      SFTerminalView *view = weakSelf; [view selectAt:point to:point kind:4]; [view notifySelection];
    }]];
    if (self.inputReady) [menu addAction:[UIAlertAction actionWithTitle:@"Paste" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) { [weakSelf pasteClipboard]; }]];
    CGPoint linkCell = CellPoint(self, point);
    if (_frame && (_frame->cells[size_t(linkCell.y) * _frame->cols + size_t(linkCell.x)].flags & 256))
      [menu addAction:[UIAlertAction actionWithTitle:@"Open link" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) { [weakSelf openLinkAt:point]; }]];
    [menu addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:nil]];
    menu.popoverPresentationController.sourceView = self;
    menu.popoverPresentationController.sourceRect = CGRectMake(point.x, point.y, 1, 1);
    [Presenter(self) presentViewController:menu animated:YES completion:nil];
  }
}
- (void)notifySelection {
  NSUInteger binding = _binding;
  __weak SFTerminalView *weakSelf = self;
  [self withTerminal:^(SFTerminal *terminal) {
    SFTextOutput text{[NSMutableData new]};
    if (sfm_terminal_copy(terminal.handle, CopyText, &text) != SFM_OK || text.failed) return;
    NSString *value = [[NSString alloc] initWithData:text.data encoding:NSUTF8StringEncoding] ?: @"";
    if (text.data.length > 65536) {
      NSMutableString *bounded = [NSMutableString new]; __block NSUInteger bytes = 0;
      [value enumerateSubstringsInRange:NSMakeRange(0, value.length) options:NSStringEnumerationByComposedCharacterSequences
        usingBlock:^(NSString *part, NSRange range, NSRange enclosing, BOOL *stop) {
          NSUInteger length = [part lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
          if (length > 65536 - bytes) { *stop = YES; return; }
          bytes += length; [bounded appendString:part];
        }];
      value = bounded;
    }
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (view && view->_binding == binding && view.selectionChanged) view.selectionChanged(value);
    });
  }];
}
- (void)copySelection {
  [self withTerminal:^(SFTerminal *terminal) {
    SFTextOutput text{[NSMutableData new]};
    if (sfm_terminal_copy(terminal.handle, CopyText, &text) != SFM_OK || text.failed) return;
    NSString *value = [[NSString alloc] initWithData:text.data encoding:NSUTF8StringEncoding];
    if (value.length) dispatch_async(dispatch_get_main_queue(), ^{
      [UIPasteboard.generalPasteboard setItems:@[@{@"public.utf8-plain-text":value}]
        options:@{UIPasteboardOptionLocalOnly:@YES}];
    });
  }];
}
- (void)pasteClipboard {
  if (!self.inputReady) return;
  NSUInteger binding = _binding;
  if (_input.markedTextRange) [_input unmarkText];
  // Only an explicit paste reads the OS clipboard. OSC output never reaches it.
  NSString *text = UIPasteboard.generalPasteboard.string;
  if (!text.length || binding != _binding || !self.window) return;
  if ([text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > 60000) {
    if (self.failed) self.failed(SFM_INVALID); return;
  }
  [self send:@{@"type":@"paste", @"text":text}];
}
- (void)fileLinkAt:(CGPoint)point {
  auto frame = _frame;
  if (!frame || !self.fileLink) return;
  CGSize size = _painter.cellSize;
  if (point.x < 0 || point.y < 0 || point.x >= frame->cols * size.width ||
      point.y >= frame->rows * size.height) return;
  CGPoint cell = CellPoint(self, point);
  NSUInteger binding = _binding;
  __weak SFTerminalView *weakSelf = self;
  [self withTerminal:^(SFTerminal *terminal) {
    if (terminal.dirty) return;
    SFTextOutput text{[NSMutableData new]};
    if (sfm_terminal_file_link(terminal.handle, (uint16_t)cell.x, (uint16_t)cell.y,
      CopyText, &text) != SFM_OK || text.failed || !text.data.length || text.data.length > 8192) return;
    NSString *target = [[NSString alloc] initWithData:text.data encoding:NSUTF8StringEncoding];
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (target && view && view->_binding == binding && view->_frame == frame && view.window && view.fileLink)
        view.fileLink(target);
    });
  }];
}
- (void)openLinkAt:(CGPoint)point {
  if (!_frame) return;
  CGPoint cell = CellPoint(self, point);
  NSUInteger binding = _binding;
  __weak SFTerminalView *weakSelf = self;
  [self withTerminal:^(SFTerminal *terminal) {
    SFTextOutput text{[NSMutableData new]};
    if (sfm_terminal_hyperlink(terminal.handle, (uint16_t)cell.x, (uint16_t)cell.y,
      CopyText, &text) != SFM_OK || text.failed || text.data.length > 4096) return;
    NSString *value = [[NSString alloc] initWithData:text.data encoding:NSUTF8StringEncoding];
    if ([value.lowercaseString hasPrefix:@"file:"]) {
      dispatch_async(dispatch_get_main_queue(), ^{
        SFTerminalView *view = weakSelf;
        if (view && view->_binding == binding && view.window && view.fileLink) view.fileLink(value);
      });
      return;
    }
    NSURLComponents *parts = value ? [NSURLComponents componentsWithString:value] : nil;
    NSString *scheme = parts.scheme.lowercaseString;
    if ((!([scheme isEqual:@"http"] || [scheme isEqual:@"https"])) ||
      !parts.host.length || parts.user || parts.password ||
      [value rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound) return;
    NSURL *url = parts.URL;
    if (!url) return;
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (!view || view->_binding != binding || !view.window) return;
      UIAlertController *alert = [UIAlertController alertControllerWithTitle:@"Open link?" message:value preferredStyle:UIAlertControllerStyleAlert];
      [alert addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:nil]];
      [alert addAction:[UIAlertAction actionWithTitle:@"Open" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) {
        [UIApplication.sharedApplication openURL:url options:@{} completionHandler:nil];
      }]];
      [Presenter(view) presentViewController:alert animated:YES completion:nil];
    });
  }];
}
@end
