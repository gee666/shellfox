// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ColorSwatches } from './ColorSwatches';

beforeEach(() => localStorage.clear());
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
function Controls() {
  const [accent, setAccent] = useState('#ec4899'), [background, setBackground] = useState('#111016');
  return <><ColorSwatches kind="accent" presets={['#ec4899']} value={accent} onChange={setAccent} />
    <ColorSwatches kind="background" presets={['#111016']} value={background} onChange={setBackground} /></>;
}
const colors = (kind: string) => within(screen.getByRole('group', { name: `Custom ${kind} colors` })).getAllByRole('button').map(button => button.getAttribute('title'));

it.each(['accent', 'background'] as const)('keeps the last three %s additions and restores them after remount', async kind => {
  const user = userEvent.setup(); const view = render(<Controls />);
  const picker = screen.getByLabelText(kind === 'accent' ? 'Accent color' : 'Background color');
  const add = screen.getByRole('button', { name: `Save custom ${kind} color` });
  expect(add).toBeDisabled();
  for (const color of ['#123456', '#234567', '#345678', '#456789']) {
    fireEvent.change(picker, { target: { value: color } });
    await user.click(add);
    expect(add).toBeDisabled();
  }
  expect(colors(kind)).toEqual(['#234567', '#345678', '#456789']);
  expect(screen.queryByRole('group', { name: `Custom ${kind === 'accent' ? 'background' : 'accent'} colors` })).not.toBeInTheDocument();
  view.unmount(); render(<Controls />);
  expect(colors(kind)).toEqual(['#234567', '#345678', '#456789']);
  await user.click(screen.getByRole('button', { name: `Use custom ${kind} #234567` }));
  expect(screen.getByLabelText(kind === 'accent' ? 'Accent color' : 'Background color')).toHaveValue('#234567');
  expect(screen.getByRole('button', { name: `Use custom ${kind} #234567` })).toHaveAttribute('aria-pressed', 'true');
  expect(colors(kind)).toEqual(['#234567', '#345678', '#456789']);
});

it('keeps the two lists independent and does not save picker drag intermediates', async () => {
  const user = userEvent.setup(); render(<Controls />);
  for (const value of ['#123456', '#234567', '#345678']) fireEvent.change(screen.getByLabelText('Accent color'), { target: { value } });
  expect(screen.queryByRole('group', { name: 'Custom accent colors' })).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Save custom accent color' }));
  fireEvent.change(screen.getByLabelText('Background color'), { target: { value: '#abcdef' } });
  await user.click(screen.getByRole('button', { name: 'Save custom background color' }));
  expect(colors('accent')).toEqual(['#345678']);
  expect(colors('background')).toEqual(['#abcdef']);
});

it('validates, normalizes and limits stored colors', () => {
  localStorage.setItem('shellfox.customColors.accent', JSON.stringify(['#ec4899', null, 12, 'bad', '#ABCDEF', '#abcdef', '#123456', '#234567', '#345678']));
  localStorage.setItem('shellfox.customColors.background', 'broken json');
  render(<Controls />);
  expect(colors('accent')).toEqual(['#123456', '#234567', '#345678']);
  expect(screen.queryByRole('group', { name: 'Custom background colors' })).not.toBeInTheDocument();
});

it('continues working when local storage is unavailable', async () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('full'); });
  const user = userEvent.setup(); render(<Controls />);
  fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#123456' } });
  await user.click(screen.getByRole('button', { name: 'Save custom accent color' }));
  expect(colors('accent')).toEqual(['#123456']);
});
