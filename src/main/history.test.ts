import { it, expect } from 'vitest';
import { historySearch } from './history';
it('binds literal substring values without interpolation', () => {
  const search = "%_' OR 1=1 --";
  const query = historySearch(search);
  expect(query.params).toEqual([search,search]);
  expect(query.sql).not.toContain(search);
  expect(query.sql).toContain('instr(lower(title), lower(?))');
});
