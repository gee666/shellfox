#pragma once
#include "shellfox_mobile.h"
#include <cstdint>
#include <stdexcept>
#include <vector>

namespace shellfox {
// Little-endian, private JNI format. Never leaves the native Android renderer.
inline void append32(std::vector<uint8_t>& out, uint32_t value) {
  for (unsigned shift = 0; shift < 32; shift += 8) out.push_back(static_cast<uint8_t>(value >> shift));
}
inline std::vector<uint8_t> copyFrame(const SfmFrame& frame) {
  const size_t count = size_t(frame.cols) * frame.rows;
  if (!frame.cols || !frame.rows || frame.cols > 512 || frame.rows > 512 ||
      count > 65536 || count != frame.cell_count || !frame.cells)
    throw std::runtime_error("Invalid terminal frame");
  std::vector<uint8_t> out;
  out.reserve(32 + count * 20);
  append32(out, 1);
  append32(out, frame.cols);
  append32(out, frame.rows);
  append32(out, frame.cursor_x);
  append32(out, frame.cursor_y);
  append32(out, frame.cursor_visible);
  for (size_t i = 0; i < count; ++i) {
    const auto& cell = frame.cells[i];
    // Bound allocations even if a future Rust ABI violates its contract.
    if (cell.text_length > 65536 || (cell.text_length && !cell.text) || cell.width > 2)
      throw std::runtime_error("Invalid terminal cell");
    append32(out, cell.foreground);
    append32(out, cell.background);
    append32(out, cell.flags);
    append32(out, cell.width);
    append32(out, static_cast<uint32_t>(cell.text_length));
    if (cell.text_length) out.insert(out.end(), cell.text, cell.text + cell.text_length);
    if (out.size() > 16 * 1024 * 1024) throw std::runtime_error("Terminal frame too large");
  }
  return out;
}
}
