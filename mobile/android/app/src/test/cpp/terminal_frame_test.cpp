#include "terminal_frame.h"
#include <cassert>
#include <iostream>

int main() {
  const uint8_t text[] = {0xe7, 0x95, 0x8c}; // UTF-8 U+754C
  SfmCell cells[] = {{text, 3, 0xabcdef, 0x123456, 5, 2}, {nullptr, 0, 0xabcdef, 0x123456, 0, 0}};
  SfmFrame frame{2, 1, 0, 0, 1, cells, 2, 1};
  const auto copied = shellfox::copyFrame(frame);
  assert(copied.size() == 24 + 40 + 3);
  assert(copied[0] == 1 && copied[4] == 2 && copied[8] == 1);
  assert(copied[24] == 0xef && copied[25] == 0xcd && copied[26] == 0xab);
  assert(copied[40] == 3 && copied[44] == 0xe7 && copied[46] == 0x8c);
  cells[0].text = nullptr;
  assert(copied[44] == 0xe7); // Callback data was copied, not retained.
  auto rejects = [](const SfmFrame& value) {
    try { shellfox::copyFrame(value); return false; } catch (const std::runtime_error&) { return true; }
  };
  assert(rejects(frame));
  cells[0].text = text;
  cells[0].width = 3;
  assert(rejects(frame));
  cells[0].width = 2;
  frame.cell_count = 1;
  assert(rejects(frame));
  frame.cell_count = 2;
  frame.cols = 513;
  assert(rejects(frame));
  std::cout << "native Android frame-copy fixtures passed\n";
}
