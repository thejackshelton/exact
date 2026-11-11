// The SVG parity smoke excuses a fixture on a host only where the schema's `hosts` column says the host does not
// draw the row the fixture exercises (LLP 1159.000 D6).
import { expect, test } from 'bun:test';
import { undrawn } from './svgparity.mjs';

test('a fixture is excused where the schema says the row is not drawn, and nowhere else', () => {
  expect(undrawn('ios')).toEqual(['fx-mix']);
  expect(undrawn('macos')).toEqual([]);
  // The paint walk's gap is every host's that draws with it.
  for (const host of ['linux', 'windows']) expect(undrawn(host)).toEqual(['fx-box-filter', 'fx-image-filter']);
  // Android's stream also has no blend.
  expect(undrawn('android')).toEqual(['fx-mix', 'fx-box-filter', 'fx-image-filter']);
  const styles = [
    { field: 'mix_blend_mode', hosts: [{ host: 'ios', fidelity: 'best-effort', why: 'x' }] },
    { field: 'filter', hosts: [{ host: 'linux', on: 'svg', fidelity: 'none', why: 'x' }] },
  ];
  // `best-effort` is compared, and an entry for another scope excuses nothing.
  expect(undrawn('ios', { styles })).toEqual([]);
  expect(undrawn('linux', { styles })).toEqual([]);
});
