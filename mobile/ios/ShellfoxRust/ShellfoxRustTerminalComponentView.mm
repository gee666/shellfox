#import "ShellfoxRustTerminalComponentView.h"
#import "SFTerminalView.h"
#include <utility>
#import <react/renderer/components/ShellfoxRustSpecs/ComponentDescriptors.h>
#import <react/renderer/components/ShellfoxRustSpecs/EventEmitters.h>
#import <react/renderer/components/ShellfoxRustSpecs/Props.h>
#import <react/renderer/components/ShellfoxRustSpecs/RCTComponentViewHelpers.h>

using namespace facebook::react;

@interface ShellfoxRustTerminalComponentView () <RCTShellfoxRustTerminalViewProtocol>
@end
@implementation ShellfoxRustTerminalComponentView {
  SFTerminalView *_terminal;
}
+ (ComponentDescriptorProvider)componentDescriptorProvider {
  return concreteComponentDescriptorProvider<ShellfoxRustTerminalComponentDescriptor>();
}
- (instancetype)initWithFrame:(CGRect)frame {
  if ((self = [super initWithFrame:frame])) {
    _props = std::make_shared<const ShellfoxRustTerminalProps>();
    _terminal = [[SFTerminalView alloc] initWithFrame:self.bounds];
    __weak ShellfoxRustTerminalComponentView *weakSelf = self;
    _terminal.stateChanged = ^(NSString *state) {
      ShellfoxRustTerminalComponentView *view = weakSelf;
      if (!view || !view->_eventEmitter) return;
      auto emitter = std::static_pointer_cast<const ShellfoxRustTerminalEventEmitter>(view->_eventEmitter);
      emitter->onState({view->_terminal.terminalHandle.UTF8String ?: "", state.UTF8String ?: "", {}});
    };
    _terminal.focusFeedback = ^(NSDictionary *result) {
      ShellfoxRustTerminalComponentView *view = weakSelf;
      if (!view || !view->_eventEmitter) return;
      auto emitter = std::static_pointer_cast<const ShellfoxRustTerminalEventEmitter>(view->_eventEmitter);
      ShellfoxRustTerminalEventEmitter::OnState event{};
      event.terminalHandle = view->_terminal.terminalHandle.UTF8String ?: "";
      event.state = "focusResult";
      decltype(event.focusResult) details{};
      details.platform = [result[@"platform"] UTF8String] ?: "";
      details.source = [result[@"source"] UTF8String] ?: "";
      details.attached = [result[@"attached"] boolValue]; details.windowFocused = [result[@"windowFocused"] boolValue];
      details.uiThread = [result[@"uiThread"] boolValue]; details.inputAllowed = [result[@"inputAllowed"] boolValue];
      details.frameAvailable = [result[@"frameAvailable"] boolValue]; details.focused = [result[@"focused"] boolValue];
      details.focusRequest = [result[@"focusRequest"] UTF8String] ?: "";
      details.keyboardRequest = [result[@"keyboardRequest"] UTF8String] ?: "";
      event.focusResult = std::move(details); emitter->onState(event);
    };
    _terminal.inputReceived = ^{
      ShellfoxRustTerminalComponentView *view = weakSelf;
      if (!view || !view->_eventEmitter) return;
      auto emitter = std::static_pointer_cast<const ShellfoxRustTerminalEventEmitter>(view->_eventEmitter);
      emitter->onInput({view->_terminal.terminalHandle.UTF8String ?: ""});
    };
    _terminal.selectionChanged = ^(NSString *text) {
      ShellfoxRustTerminalComponentView *view = weakSelf;
      if (!view || !view->_eventEmitter) return;
      auto emitter = std::static_pointer_cast<const ShellfoxRustTerminalEventEmitter>(view->_eventEmitter);
      NSData *utf8 = [text dataUsingEncoding:NSUTF8StringEncoding];
      emitter->onSelection({view->_terminal.terminalHandle.UTF8String ?: "",
        utf8.length ? std::string((const char *)utf8.bytes, utf8.length) : std::string()});
    };
    _terminal.failed = ^(SfmStatus status) {
      ShellfoxRustTerminalComponentView *view = weakSelf;
      if (!view || !view->_eventEmitter) return;
      auto emitter = std::static_pointer_cast<const ShellfoxRustTerminalEventEmitter>(view->_eventEmitter);
      emitter->onError({view->_terminal.terminalHandle.UTF8String ?: "", status});
    };
    self.contentView = _terminal;
  }
  return self;
}
- (NSObject *)accessibilityElement { return _terminal; }
- (void)updateProps:(const Props::Shared &)props oldProps:(const Props::Shared &)oldProps {
  const auto &next = *std::static_pointer_cast<const ShellfoxRustTerminalProps>(props);
  _terminal.readOnly = next.readOnly;
  _terminal.focused = next.focused;
  _terminal.selecting = next.selecting;
  _terminal.modifiers = next.modifiers;
  _terminal.fontSize = next.fontSize > 0 ? next.fontSize : 14;
  if (next.terminalHandle.size() > 20) {
    _terminal.terminalHandle = @"";
    if (_terminal.failed) _terminal.failed(SFM_INVALID);
  } else {
    _terminal.terminalHandle = [[NSString alloc] initWithBytes:next.terminalHandle.data()
      length:next.terminalHandle.size() encoding:NSUTF8StringEncoding] ?: @"";
  }
  [super updateProps:props oldProps:oldProps];
}
- (void)prepareForRecycle {
  [_terminal detach];
  _terminal.terminalHandle = @"";
  _terminal.modifiers = 0;
  _terminal.focused = NO;
  _terminal.selecting = NO;
  [super prepareForRecycle];
}
@end
