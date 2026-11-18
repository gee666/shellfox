#import "SFTerminalView.h"
#include <algorithm>
#include <cmath>

@interface SFDisplayTarget : NSObject
@property(nonatomic, weak) SFTerminalView *view;
- (void)tick:(CADisplayLink *)link;
@end
@interface SFTerminalView ()
- (void)displayTick;
@end
@implementation SFDisplayTarget
- (void)tick:(CADisplayLink *)link { [self.view displayTick]; }
@end

@implementation SFTerminalView {
  CADisplayLink *_displayLink;
  SFDisplayTarget *_displayTarget;
  CGSize _lastGrid;
  NSString *_accessibleText;
  uint64_t _frameEpoch;
  BOOL _parserReady;
}
- (instancetype)initWithFrame:(CGRect)frame {
  if ((self = [super initWithFrame:frame])) {
    self.backgroundColor = UIColor.blackColor;
    self.opaque = YES;
    self.clipsToBounds = YES;
    self.isAccessibilityElement = YES;
    self.accessibilityLabel = @"Terminal";
    self.accessibilityTraits = UIAccessibilityTraitUpdatesFrequently;
    _painter = [SFFramePainter new];
    _input = [[SFTextInput alloc] initWithFrame:CGRectZero textContainer:nil];
    _input.isAccessibilityElement = NO;
    _input.editable = NO;
    __weak SFTerminalView *weakSelf = self;
    _input.sendEvent = ^(NSDictionary *event) { [weakSelf send:event]; };
    _input.requestPaste = ^{ [weakSelf pasteClipboard]; };
    _input.requestCopy = ^{ [weakSelf copySelection]; };
    [self addSubview:_input];
    [self installGestures];
    [NSNotificationCenter.defaultCenter addObserver:self selector:@selector(cancelInput)
      name:UIApplicationWillResignActiveNotification object:nil];
    [NSNotificationCenter.defaultCenter addObserver:self selector:@selector(background)
      name:UIApplicationDidEnterBackgroundNotification object:nil];
    [NSNotificationCenter.defaultCenter addObserver:self selector:@selector(foreground)
      name:UIApplicationWillEnterForegroundNotification object:nil];
    self.accessibilityCustomActions = @[
      [[UIAccessibilityCustomAction alloc] initWithName:@"Copy selection" target:self selector:@selector(accessibleCopy)],
      [[UIAccessibilityCustomAction alloc] initWithName:@"Paste" target:self selector:@selector(accessiblePaste)],
      [[UIAccessibilityCustomAction alloc] initWithName:@"Show keyboard" target:self selector:@selector(accessibilityActivate)]];
  }
  return self;
}
- (void)dealloc {
  if (_mailbox) { _mailbox->active.store(false); _mailbox->resetFrame(); }
  [_displayLink invalidate];
  [NSNotificationCenter.defaultCenter removeObserver:self];
}
- (void)setTerminalHandle:(NSString *)handle {
  uint64_t parsed = 0;
  if (handle.length && !SFParseId(handle, &parsed)) {
    [self detach]; _terminalHandle = @"";
    if (self.failed) self.failed(SFM_INVALID); return;
  }
  if ([_terminalHandle isEqualToString:handle]) return;
  [self detach];
  _terminalHandle = [handle copy];
  if (self.window) [self bind];
}
- (BOOL)inputReady {
  return _parserReady && !self.readOnly && self.window && _mailbox &&
    !_mailbox->deliveryOnly.load() && _mailbox->active.load();
}
- (void)setReadOnly:(BOOL)value {
  if (_readOnly == value) return;
  _readOnly = value;
  _input.editable = self.inputReady;
  if (value) { [_input cancelComposition]; [_input resignFirstResponder]; }
  else if (self.inputReady && self.focused) [self focusInputForAction:SFFocusReadiness attempt:YES];
  [self withTerminal:^(SFTerminal *terminal) {
    terminal.readOnly = value;
    if (!value && !terminal.recovering)
      [[SFOwner shared] resize:terminal.identifier columns:terminal.viewportColumns rows:terminal.viewportRows];
  }];
}
- (void)setFocused:(BOOL)value {
  if (_focused == value) return;
  _focused = value;
  if (value && self.inputReady) [self focusInputForAction:SFFocusKeyboard attempt:YES];
  else { [_input cancelComposition]; [_input resignFirstResponder];
    if (value) [self focusInputForAction:SFFocusKeyboard attempt:NO];
  }
}
- (void)setSelecting:(BOOL)value {
  if (_selecting == value) return;
  _selecting = value;
  if (!value) {
    [self selectAt:CGPointZero to:CGPointZero kind:0];
    if (self.selectionChanged) self.selectionChanged(@"");
  }
}
- (void)setModifiers:(NSInteger)value { _modifiers = value >= 0 && value <= 15 ? value : 0; }
- (void)setFontSize:(CGFloat)value {
  CGFloat bounded = std::isfinite(value) ? std::clamp(value, CGFloat(8), CGFloat(40)) : 14;
  if (_fontSize == bounded) return;
  _fontSize = bounded;
  _painter.fontSize = _fontSize;
  _input.font = [UIFont monospacedSystemFontOfSize:_fontSize weight:UIFontWeightRegular];
  _lastGrid = CGSizeZero;
  [self setNeedsLayout]; [self setNeedsDisplay];
}
- (void)didMoveToWindow {
  [super didMoveToWindow];
  if (self.window) {
    self.contentScaleFactor = self.window.screen.scale;
    if (!_displayLink) {
      _displayTarget = [SFDisplayTarget new]; _displayTarget.view = self;
      _displayLink = [CADisplayLink displayLinkWithTarget:_displayTarget selector:@selector(tick:)];
      [_displayLink addToRunLoop:NSRunLoop.mainRunLoop forMode:NSRunLoopCommonModes];
    }
    [self bind];
  } else {
    [self detach]; [_displayLink invalidate]; _displayLink = nil;
  }
}
- (void)cancelInput { [_input cancelComposition]; }
- (void)background { [self detach]; }
- (void)foreground { if (self.window) [self bind]; }
- (void)bind {
  if (!_terminalHandle.length || _mailbox) return;
  NSUInteger binding = ++_binding;
  NSString *handle = _terminalHandle;
  BOOL readOnly = self.readOnly;
  // Publish the claim before queueing. Detach can cancel even an unfinished bind.
  auto mailbox = std::make_shared<shellfox::Mailbox>();
  mailbox->active.store(true); _mailbox = mailbox;
  __weak SFTerminalView *weakSelf = self;
  BOOL accepted = [[SFOwner shared] enqueue:^{
    if (!mailbox->active.load()) return;
    SFTerminal *terminal = [[SFOwner shared] terminal:handle];
    SfmStatus status = terminal ? SFM_OK : SFM_STALE;
    if (terminal && terminal.mailbox->active.load()) status = SFM_BUSY;
    if (status == SFM_OK) {
      mailbox->setAppearance(terminal.mailbox->appearance());
      mailbox->ready.store(terminal.mailbox->ready.load());
      mailbox->deliveryOnly.store(terminal.deliveryOnly);
      mailbox->hostEncoding.store(terminal.hostEncoding);
      terminal.mailbox = mailbox; terminal.readOnly = readOnly;
      terminal.visible = YES; terminal.dirty = YES;
    }
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (!view || view->_binding != binding || !view.window) {
        if (status == SFM_OK) mailbox->active.store(false);
        return;
      }
      if (status != SFM_OK) {
        mailbox->active.store(false); view->_mailbox.reset();
        if (view.failed) view.failed(status); return;
      }
      view->_lastGrid = CGSizeZero;
      [view setNeedsLayout];
      if (view.stateChanged) view.stateChanged(@"bound");
    });
  }];
  if (!accepted) {
    mailbox->active.store(false); _mailbox.reset();
    if (self.failed) self.failed(SFM_BUSY);
  }
}
- (void)detach {
  ++_binding;
  [_input cancelComposition]; [_input resignFirstResponder];
  if (_mailbox) { _mailbox->active.store(false); _mailbox->resetFrame(); }
  _mailbox.reset(); _frame.reset(); _accessibleText = nil;
  _parserReady = NO; _input.editable = NO;
  [self setNeedsDisplay];
}
- (void)displayTick {
  if (!_mailbox) return;
  if (_mailbox->closed.load()) {
    [self detach]; if (self.stateChanged) self.stateChanged(@"closed"); return;
  }
  _input.hostEncoding = _mailbox->hostEncoding.load();
  BOOL ready = _mailbox->ready.load();
  if (ready != _parserReady) {
    _parserReady = ready; _input.editable = self.inputReady;
    if (!ready) { [_input cancelComposition]; [_input resignFirstResponder]; }
    else if (self.focused && self.inputReady) [self focusInputForAction:SFFocusReadiness attempt:YES];
    if (self.stateChanged) self.stateChanged(ready ? @"ready" : @"recovering");
  }
  auto appearance = _mailbox->appearance();
  if (!(appearance == _painter.appearance)) {
    _painter.appearance = appearance;
    self.backgroundColor = SFColor(appearance.background);
    _input.markedTextStyle = @{NSForegroundColorAttributeName:SFColor(appearance.foreground),
      NSBackgroundColorAttributeName:SFColor(appearance.background), NSUnderlineStyleAttributeName:@1};
    [self setNeedsDisplay];
  }
  SfmStatus failure = _mailbox->failure.exchange(SFM_OK);
  if (failure != SFM_OK && self.failed) self.failed(failure);
  uint64_t epoch = _mailbox->epoch.load();
  if (epoch != _frameEpoch) {
    _frameEpoch = epoch; _frame.reset(); _accessibleText = nil;
    [self setNeedsDisplay];
  }
  auto next = _mailbox->take();
  if (!next) return;
  _frame = std::move(next);
  _accessibleText = nil;
  [self positionInput];
  [self setNeedsDisplay];
}
- (void)positionInput {
  CGFloat x = _frame ? _frame->cursorX * _painter.cellSize.width : 0;
  CGFloat y = _frame ? _frame->cursorY * _painter.cellSize.height : 0;
  _input.frame = CGRectMake(x, y, MAX(_painter.cellSize.width, self.bounds.size.width - x), _painter.cellSize.height);
}
- (void)layoutSubviews {
  [super layoutSubviews];
  [self positionInput];
  if (!_mailbox || self.bounds.size.width < 1 || self.bounds.size.height < 1) return;
  uint16_t cols = (uint16_t)std::clamp(floor(self.bounds.size.width / _painter.cellSize.width), 2.0, 500.0);
  uint16_t rows = (uint16_t)std::clamp(floor(self.bounds.size.height / _painter.cellSize.height), 2.0, 500.0);
  rows = std::min(rows, uint16_t(65536 / cols));
  CGSize grid = CGSizeMake(cols, rows);
  if (CGSizeEqualToSize(grid, _lastGrid)) return;
  _lastGrid = grid;
  // Native layout, including the React safe-area/keyboard container, owns DPI and grid size.
  [self withTerminal:^(SFTerminal *terminal) {
    SfmStatus status = [[SFOwner shared] resize:terminal.identifier columns:cols rows:rows];
    if (status != SFM_OK) [terminal.sink terminalFailed:terminal.identifier status:status];
  }];
}
- (void)drawRect:(CGRect)rect {
  CGContextRef context = UIGraphicsGetCurrentContext();
  if (!context) return;
  CGContextSetFillColorWithColor(context, SFColor(_painter.appearance.background).CGColor);
  CGContextFillRect(context, self.bounds);
  if (_frame) [_painter draw:*_frame bounds:self.bounds context:context cursor:YES];
}
- (NSString *)accessibilityValue {
  if (!_frame) return @"";
  if (!_accessibleText) _accessibleText = [_painter text:*_frame range:NSMakeRange(0, _frame->cells.size())];
  return _accessibleText;
}
- (BOOL)accessibilityActivate {
  return [self focusInputForAction:SFFocusAccessibility attempt:self.inputReady];
}
- (BOOL)accessibleCopy { [self copySelection]; return YES; }
- (BOOL)accessiblePaste { if (!self.inputReady) return NO; [self pasteClipboard]; return YES; }
- (BOOL)accessibilityScroll:(UIAccessibilityScrollDirection)direction {
  if (!_frame || (direction != UIAccessibilityScrollDirectionUp && direction != UIAccessibilityScrollDirectionDown)) return NO;
  [self scrollRows:direction == UIAccessibilityScrollDirectionUp ? -int32_t(_frame->rows) : int32_t(_frame->rows)];
  UIAccessibilityPostNotification(UIAccessibilityPageScrolledNotification, nil);
  return YES;
}
- (void)withTerminal:(void (^)(SFTerminal *))work {
  NSString *handle = _terminalHandle;
  auto mailbox = _mailbox;
  if (!mailbox || !mailbox->active.load()) return;
  BOOL accepted = [[SFOwner shared] enqueue:^{
    SFTerminal *terminal = [[SFOwner shared] terminal:handle];
    if (terminal && terminal.mailbox == mailbox && mailbox->active.load()) work(terminal);
  }];
  if (!accepted && self.failed) self.failed(SFM_BUSY);
}
- (void)send:(NSDictionary *)event {
  BOOL release = [event[@"action"] isEqual:@"release"] || ([event[@"kind"] isEqual:@"key"] && [event[@"event"][@"action"] isEqual:@"release"]);
  if (self.readOnly && !release) return;
  NSMutableDictionary *encoded = [event mutableCopy];
  if (_input.hostEncoding && [event[@"type"] isEqual:@"commit"])
    encoded = [SFManagerLogicalKey(event[@"text"], self.modifiers) mutableCopy];
  if ([event[@"kind"] isEqual:@"key"]) {
    NSMutableDictionary *key = [event[@"event"] mutableCopy];
    key[@"modifiers"] = @([key[@"modifiers"] unsignedIntegerValue] | self.modifiers);
    encoded[@"event"] = key;
  }
  if ([event[@"type"] isEqual:@"key"] || [event[@"type"] isEqual:@"mouse"])
    encoded[@"modifiers"] = @([event[@"modifiers"] unsignedIntegerValue] | self.modifiers);
  else if (self.modifiers && [event[@"type"] isEqual:@"commit"]) {
    NSString *text = event[@"text"];
    if (text.length == 1 && [text characterAtIndex:0] >= 32 && [text characterAtIndex:0] < 127)
      encoded = [@{@"type":@"key", @"key":@{@"kind":@"character", @"value":text},
        @"action":@"press", @"modifiers":@(self.modifiers)} mutableCopy];
  }
  __weak SFTerminalView *weakSelf = self;
  NSUInteger binding = _binding;
  void (^work)(SFTerminal *) = ^(SFTerminal *terminal) {
    SfmStatus status = [[SFOwner shared] input:terminal.identifier event:encoded];
    BOOL userInput = ![event[@"type"] isEqual:@"preedit"] && !release;
    dispatch_async(dispatch_get_main_queue(), ^{
      SFTerminalView *view = weakSelf;
      if (!view || view->_binding != binding || !view.window) return;
      if (status != SFM_OK) { if (view.failed) view.failed(status); }
      else if (userInput && view.inputReceived) view.inputReceived();
    });
  };
  if (release && _mailbox) {
    // Focus-loss releases target the old binding even after its viewport detaches.
    NSString *handle = _terminalHandle; auto mailbox = _mailbox;
    BOOL accepted = [[SFOwner shared] enqueue:^{
      SFTerminal *terminal = [[SFOwner shared] terminal:handle];
      if (terminal && terminal.mailbox == mailbox) work(terminal);
    }];
    if (!accepted && self.failed) self.failed(SFM_BUSY);
  } else [self withTerminal:work];
}
@end
