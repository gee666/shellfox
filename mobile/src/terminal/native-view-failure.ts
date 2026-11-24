/**
 * What a native view error code means. BUSY, STALE and EMPTY (1, 3, 7) are
 * input the terminal could not take while it reattaches: dropped, as 0.9.15
 * dropped it. INVALID (2) is a paste that is too large. Anything else is an
 * engine failure: the view reopens its terminal.
 */
export function nativeViewFailure(code: number): 'dropped' | 'paste' | 'engine' {
  if (code === 1 || code === 3 || code === 7) return 'dropped';
  return code === 2 ? 'paste' : 'engine';
}
