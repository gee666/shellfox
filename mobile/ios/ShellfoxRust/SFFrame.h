#pragma once
#import <UIKit/UIKit.h>
#include <atomic>
#include <memory>
#include <mutex>
#include <utility>
#include <string>
#include <vector>
#include "shellfox_mobile.h"

namespace shellfox {
struct Appearance {
  uint32_t foreground = 0xFFFFFF, background = 0, cursor = 0xFFFFFF;
  uint32_t selectionBackground = 0x335599, selectionForeground = 0xFFFFFF;
  bool operator==(const Appearance &) const = default;
};
struct Cell {
  std::string text;
  uint32_t foreground, background;
  uint16_t flags;
  uint8_t width;
};
struct Frame {
  uint16_t cols, rows, cursorX, cursorY;
  bool cursorVisible;
  uint64_t consumed;
  std::vector<Cell> cells;
  static std::shared_ptr<const Frame> copy(const SfmFrame &source);
};
// One replaceable pending frame, one frame retained by UIKit. No frame backlog.
class Mailbox {
  std::mutex mutex_;
  std::shared_ptr<const Frame> pending_;
  Appearance appearance_;
public:
  std::atomic<bool> active{false};
  std::atomic<bool> closed{false};
  std::atomic<bool> ready{false};
  std::atomic<bool> deliveryOnly{false};
  std::atomic<bool> hostEncoding{false};
  std::atomic<uint64_t> epoch{0};
  std::atomic<SfmStatus> failure{SFM_OK};
  void put(std::shared_ptr<const Frame> frame);
  std::shared_ptr<const Frame> take();
  void resetFrame();
  void setAppearance(Appearance appearance);
  Appearance appearance();
};
}

UIColor *SFColor(uint32_t rgb);
@interface SFFramePainter : NSObject
@property(nonatomic) CGFloat fontSize;
@property(nonatomic) shellfox::Appearance appearance;
@property(nonatomic, readonly) CGSize cellSize;
- (void)draw:(const shellfox::Frame &)frame bounds:(CGRect)bounds
    context:(CGContextRef)context cursor:(BOOL)cursor;
- (NSString *)text:(const shellfox::Frame &)frame range:(NSRange)range;
@end
