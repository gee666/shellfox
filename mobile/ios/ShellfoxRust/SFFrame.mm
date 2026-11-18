#import "SFFrame.h"
#import <CoreText/CoreText.h>
#include <algorithm>
#include <cmath>
#include <limits>
#include <mutex>

namespace shellfox {
std::shared_ptr<const Frame> Frame::copy(const SfmFrame &source) {
  if (!source.cols || !source.rows || source.cols > 512 || source.rows > 512 ||
      source.cell_count != size_t(source.cols) * source.rows || source.cell_count > 65536 ||
      !source.cells) return nullptr;
  auto result = std::make_shared<Frame>();
  result->cols = source.cols; result->rows = source.rows;
  result->cursorX = source.cursor_x; result->cursorY = source.cursor_y;
  result->cursorVisible = source.cursor_visible != 0; result->consumed = source.consumed_sequence;
  result->cells.reserve(source.cell_count);
  size_t bytes = 0;
  for (size_t i = 0; i < source.cell_count; ++i) {
    const auto &cell = source.cells[i];
    bytes += cell.text_length;
    if (cell.width > 2 || cell.text_length > 65536 || bytes > SFM_MAX_QUEUE_BYTES ||
        (cell.text_length && !cell.text)) return nullptr;
    result->cells.push_back({cell.text_length ? std::string((const char *)cell.text, cell.text_length) : "",
      cell.foreground, cell.background, cell.flags, cell.width});
  }
  return result;
}
void Mailbox::put(std::shared_ptr<const Frame> frame) {
  std::lock_guard<std::mutex> guard(mutex_); pending_ = std::move(frame);
}
std::shared_ptr<const Frame> Mailbox::take() {
  std::lock_guard<std::mutex> guard(mutex_); return std::exchange(pending_, nullptr);
}
void Mailbox::resetFrame() {
  std::lock_guard<std::mutex> guard(mutex_); pending_.reset(); epoch.fetch_add(1);
}
void Mailbox::setAppearance(Appearance appearance) {
  std::lock_guard<std::mutex> guard(mutex_); appearance_ = appearance;
}
Appearance Mailbox::appearance() {
  std::lock_guard<std::mutex> guard(mutex_); return appearance_;
}
}

UIColor *SFColor(uint32_t rgb) {
  return [UIColor colorWithRed:((rgb >> 16) & 255) / 255.0
    green:((rgb >> 8) & 255) / 255.0 blue:(rgb & 255) / 255.0 alpha:1];
}
static NSString *String(const shellfox::Cell &cell) {
  return [[NSString alloc] initWithBytes:cell.text.data() length:cell.text.size()
                              encoding:NSUTF8StringEncoding] ?: @"\uFFFD";
}

@implementation SFFramePainter {
  UIFont *_font;
  CGSize _cellSize;
  NSArray *_fonts;
}
- (instancetype)init {
  if ((self = [super init])) self.fontSize = 14;
  return self;
}
- (void)setFontSize:(CGFloat)fontSize {
  _fontSize = std::clamp(fontSize, CGFloat(8), CGFloat(40));
  _font = [UIFont monospacedSystemFontOfSize:_fontSize weight:UIFontWeightRegular];
  NSMutableArray *fonts = [NSMutableArray new];
  for (unsigned style = 0; style < 4; ++style) {
    UIFontDescriptorSymbolicTraits traits = (style & 1 ? UIFontDescriptorTraitBold : 0) |
      (style & 2 ? UIFontDescriptorTraitItalic : 0);
    UIFontDescriptor *descriptor = [_font.fontDescriptor fontDescriptorWithSymbolicTraits:traits];
    UIFont *font = descriptor ? [UIFont fontWithDescriptor:descriptor size:_fontSize] : _font;
    CTFontRef ctFont = CTFontCreateWithName((__bridge CFStringRef)font.fontName, _fontSize, nullptr);
    [fonts addObject:(__bridge id)ctFont]; CFRelease(ctFont);
  }
  _fonts = fonts;
  _cellSize = CGSizeMake([@"M" sizeWithAttributes:@{NSFontAttributeName:_font}].width,
                        ceil(_font.lineHeight));
}
- (CGSize)cellSize { return _cellSize; }
- (void)draw:(const shellfox::Frame &)frame bounds:(CGRect)bounds
    context:(CGContextRef)context cursor:(BOOL)cursor {
  CGContextSaveGState(context);
  CGContextClipToRect(context, bounds);
  CGAffineTransform transform = CGContextGetCTM(context);
  const CGFloat scale = MAX(1.0, hypot(transform.a, transform.b));
  auto rect = [&](size_t index, unsigned width) {
    return CGRectMake(round((index % frame.cols) * _cellSize.width * scale) / scale,
      round((index / frame.cols) * _cellSize.height * scale) / scale,
      ceil(width * _cellSize.width * scale) / scale, _cellSize.height);
  };
  for (size_t i = 0; i < frame.cells.size(); ++i) {
    const auto &cell = frame.cells[i];
    bool inverse = cell.flags & 8;
    UIColor *background = SFColor(inverse ? cell.foreground : cell.background);
    if (cell.flags & 128) background = SFColor(_appearance.selectionBackground);
    CGContextSetFillColorWithColor(context, background.CGColor);
    CGContextFillRect(context, rect(i, 1));
  }
  for (size_t row = 0; row < frame.rows; ++row) {
    NSMutableAttributedString *text = [NSMutableAttributedString new];
    std::vector<size_t> anchors;
    for (size_t col = 0; col < frame.cols; ++col) {
      size_t i = row * frame.cols + col;
      const auto &cell = frame.cells[i];
      if (!cell.width) continue;
      UIColor *foreground = SFColor(cell.flags & 128 ? _appearance.selectionForeground :
        (cell.flags & 8 ? cell.background : cell.foreground));
      if (cell.flags & 16) foreground = [foreground colorWithAlphaComponent:0.5];
      NSDictionary *attributes = @{(__bridge NSString *)kCTFontAttributeName:_fonts[cell.flags & 3],
        (__bridge NSString *)kCTForegroundColorAttributeName:(__bridge id)foreground.CGColor,
        NSLigatureAttributeName:@0};
      NSString *value = cell.flags & 32 ? @" " : String(cell);
      for (NSUInteger unit = 0; unit < value.length; ++unit) anchors.push_back(i);
      [text appendAttributedString:[[NSAttributedString alloc] initWithString:value attributes:attributes]];
      CGRect box = rect(i, cell.width);
      CGContextSetFillColorWithColor(context, foreground.CGColor);
      if ((cell.flags & 4) && !(cell.flags & 32)) CGContextFillRect(context, CGRectMake(box.origin.x, CGRectGetMaxY(box) - 2, box.size.width, 1 / scale));
      if ((cell.flags & 64) && !(cell.flags & 32)) CGContextFillRect(context, CGRectMake(box.origin.x, box.origin.y + box.size.height / 2, box.size.width, 1 / scale));
    }
    // Shape the whole row, then anchor shaped clusters to engine cell coordinates.
    // CoreText chooses fallback fonts; UTF-16 indices keep wide/combining cells aligned.
    CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)text);
    CFArrayRef runs = CTLineGetGlyphRuns(line);
    std::vector<CGFloat> origins(frame.cols, std::numeric_limits<CGFloat>::infinity());
    for (CFIndex r = 0; r < CFArrayGetCount(runs); ++r) {
      CTRunRef run = (CTRunRef)CFArrayGetValueAtIndex(runs, r);
      CFIndex count = CTRunGetGlyphCount(run);
      std::vector<CGPoint> positions(count); std::vector<CFIndex> indices(count);
      CTRunGetPositions(run, CFRangeMake(0, count), positions.data());
      CTRunGetStringIndices(run, CFRangeMake(0, count), indices.data());
      for (CFIndex g = 0; g < count; ++g) if (indices[g] >= 0 && size_t(indices[g]) < anchors.size()) {
        size_t col = anchors[indices[g]] % frame.cols;
        origins[col] = std::min(origins[col], positions[g].x);
      }
    }
    CGContextSaveGState(context);
    CGContextTranslateCTM(context, 0, row * _cellSize.height + _font.ascender);
    CGContextScaleCTM(context, 1, -1);
    CGContextSetTextMatrix(context, CGAffineTransformIdentity);
    for (CFIndex r = 0; r < CFArrayGetCount(runs); ++r) {
      CTRunRef run = (CTRunRef)CFArrayGetValueAtIndex(runs, r);
      CFIndex count = CTRunGetGlyphCount(run);
      std::vector<CGGlyph> glyphs(count);
      std::vector<CGPoint> positions(count);
      std::vector<CFIndex> indices(count);
      CTRunGetGlyphs(run, CFRangeMake(0, count), glyphs.data());
      CTRunGetPositions(run, CFRangeMake(0, count), positions.data());
      CTRunGetStringIndices(run, CFRangeMake(0, count), indices.data());
      NSDictionary *attrs = (__bridge NSDictionary *)CTRunGetAttributes(run);
      CTFontRef font = (__bridge CTFontRef)attrs[(__bridge NSString *)kCTFontAttributeName];
      CGColorRef color = (__bridge CGColorRef)attrs[(__bridge NSString *)kCTForegroundColorAttributeName];
      if (!font || !color) continue;
      CGContextSetFillColorWithColor(context, color);
      for (CFIndex g = 0; g < count; ++g) {
        if (indices[g] < 0 || size_t(indices[g]) >= anchors.size()) continue;
        size_t col = anchors[indices[g]] % frame.cols;
        positions[g].x += col * _cellSize.width - origins[col];
      }
      CTFontDrawGlyphs(font, glyphs.data(), positions.data(), count, context);
    }
    CGContextRestoreGState(context);
    CFRelease(line);
  }
  if (cursor && frame.cursorVisible && frame.cursorX < frame.cols && frame.cursorY < frame.rows) {
    size_t index = size_t(frame.cursorY) * frame.cols + frame.cursorX;
    CGRect box = rect(index, std::max(unsigned(frame.cells[index].width), 1u));
    CGContextSetFillColorWithColor(context, [SFColor(_appearance.cursor) colorWithAlphaComponent:0.6].CGColor);
    CGContextFillRect(context, box);
  }
  CGContextRestoreGState(context);
}
- (NSString *)text:(const shellfox::Frame &)frame range:(NSRange)range {
  NSMutableString *text = [NSMutableString new];
  size_t begin = std::min(size_t(range.location), frame.cells.size());
  size_t end = begin + std::min(size_t(range.length), frame.cells.size() - begin);
  for (size_t i = begin; i < end; ++i) {
    if (i > begin && i % frame.cols == 0) [text appendString:@"\n"];
    if (frame.cells[i].width) [text appendString:String(frame.cells[i])];
  }
  return text;
}
@end
