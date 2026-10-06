// instr implements literal substring matching, including %, _ and backslash.
export function historySearch(search: string): { sql: string; params: string[] } {
  return { sql: "settledAt IS NOT NULL AND (instr(lower(title), lower(?)) > 0 OR instr(lower(cwd), lower(?)) > 0)", params: [search, search] };
}
