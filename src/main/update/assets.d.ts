// Update scripts are bundled as text (esbuild loader / vitest transform).
declare module '*.sh' { const text: string; export default text; }
declare module '*.ps1' { const text: string; export default text; }
