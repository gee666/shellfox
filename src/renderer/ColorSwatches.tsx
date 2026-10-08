import { useState } from 'react';
import { Icon } from './components';

type ColorKind = 'accent' | 'background';
const colorPattern = /^#[0-9a-f]{6}$/i;
const storageKey = (kind: ColorKind) => `shellfox.customColors.${kind}`;

function readColors(kind: ColorKind, presets: readonly string[]): string[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(storageKey(kind)) ?? '[]');
    if (!Array.isArray(stored)) return [];
    return [...new Set(stored.filter((color): color is string => typeof color === 'string' && colorPattern.test(color))
      .map(color => color.toLowerCase()).filter(color => !presets.includes(color)))].slice(-3);
  } catch { return []; }
}

export function ColorSwatches({ kind, presets, value, onChange }: {
  kind: ColorKind; presets: readonly string[]; value: string; onChange: (color: string) => void;
}) {
  const [custom, setCustom] = useState(() => readColors(kind, presets));
  const color = value.toLowerCase();
  const canAdd = colorPattern.test(color) && !presets.includes(color) && !custom.includes(color);
  function add() {
    if (!canAdd) return;
    const next = [...custom, color].slice(-3);
    setCustom(next);
    try { localStorage.setItem(storageKey(kind), JSON.stringify(next)); } catch { /* Keep swatches usable if storage is unavailable. */ }
  }
  const swatch = (item: string, saved = false) => <button key={item} type="button" className="swatch" style={{ backgroundColor: item }}
    aria-label={saved ? `Use custom ${kind} ${item}` : kind === 'accent' ? `Use ${item}` : `Use background ${item}`}
    title={item} aria-pressed={color === item} onClick={() => onChange(item)} />;
  return <div className="swatches">
    {presets.map(item => swatch(item))}
    {custom.length > 0 && <span className="custom-swatches" role="group" aria-label={`Custom ${kind} colors`}>{custom.map(item => swatch(item, true))}</span>}
    <input type="color" aria-label={kind === 'accent' ? 'Accent color' : 'Background color'} title={`Custom ${kind} color`} value={value} onChange={event => onChange(event.target.value)} />
    <button type="button" className="icon-button" aria-label={`Save custom ${kind} color`} title="Save color. Keeps the last 3." disabled={!canAdd} onClick={add}><Icon name="plus" /></button>
  </div>;
}
