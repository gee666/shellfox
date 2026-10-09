import { expect, it } from 'vitest';
import { TAB_WINDOW_WIDTH, tabWindowIndices } from './terminal-tab-window';

it('bounds work by viewport width and retained interaction targets, not total tab count', () => {
  const visible = tabWindowIndices(100000, 50000 * TAB_WINDOW_WIDTH, 800, [0, 99999, 100, 100, -1, 100000]);
  expect(visible.length).toBeLessThan(20);
  expect(visible).toContain(0); expect(visible).toContain(100); expect(visible).toContain(99999);
  expect(new Set(visible).size).toBe(visible.length);
  expect(visible).toEqual([...visible].sort((a, b) => a - b));
});
it('keeps both ends within the logical list and includes partial slots with overscan', () => {
  expect(tabWindowIndices(0, 0, 800, [])).toEqual([]);
  expect(tabWindowIndices(3, 0, 800, [])).toEqual([0, 1, 2]);
  expect(tabWindowIndices(1000, 999 * TAB_WINDOW_WIDTH, 800, [])).toEqual([992, 993, 994, 995, 996, 997, 998, 999]);
  expect(tabWindowIndices(200, 999 * TAB_WINDOW_WIDTH, 800, [])).toEqual([192, 193, 194, 195, 196, 197, 198, 199]);
  expect(tabWindowIndices(1000, 5 * TAB_WINDOW_WIDTH + 10, 180, [])).toEqual([2, 3, 4, 5, 6, 7, 8, 9]);
});
