import { describe, expect, it } from 'vitest';
import { durationText as fromExplain, payoffStructure } from '../explain/models';
import { durationText } from './models';

describe('schematics/models (spec 7b §7.2)', () => {
  it('explain/models re-exports the moved durationText, so the panel is unchanged', () => {
    expect(fromExplain).toBe(durationText);
    expect(payoffStructure('yes', 3, true).broken).toEqual([false, false, false, true]);
  });

  it('reads the example and a still-open row', () => {
    expect(durationText({ kind: 'example', label: 'example', ms: 3800, open: false, ticks: [] })).toBe('example: 3.80 s, from u2 to u5, peak at u3');
    expect(durationText({ kind: 'row', label: 'Fed', ms: 125_000, open: true, ticks: [] })).toBe('Fed: still open, 2 min 5 s so far');
    expect(durationText({ kind: 'row', label: 'Fed', ms: 4190, open: false, ticks: [] })).toBe('Fed: 4.19 s');
  });
});
