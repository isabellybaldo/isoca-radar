import { describe, expect, it } from 'vitest';
import cardsAll from '../frozen/cards-all.json?raw';
import publicJson from '../frozen/public.json?raw';
import type { CardsResponse } from './api/types';

// The portfolio site's frozen data (spec 2026-10-24 §5.1): exported once from the old VPS, never edited.
// Only the ALL window ships: 7D and 30D hold the same three days and were still 'warming' when capture stopped.
const RAW: Record<string, string> = { 'public.json': publicJson, 'cards-all.json': cardsAll };
const read = (f: string) => RAW[f];
const FILES = Object.keys(RAW);

describe('frozen data', () => {
  it('cards-all.json is a CardsResponse with a final SWEET card', () => {
    const r = JSON.parse(read('cards-all.json')) as CardsResponse;
    expect(r.win).toBe('all');
    const sweet = r.cards.find((c) => c.card === 'SWEET');
    expect(sweet?.series.sweet).toBeDefined();
    expect(sweet?.status).toBe('live');
  });
  it('public.json is a v1 snapshot under 64 KB', () => {
    const s = read('public.json');
    expect(s.length).toBeLessThan(65536);
    expect(JSON.parse(s).v).toBe(1);
  });
  it.each(FILES)('%s names neither the old VPS nor an account', (f) => {
    const s = read(f);
    for (const bad of ['31.97.23.123', 'srv1125993', 'account_id', 'isoca-radar-scanner']) expect(s).not.toContain(bad);
  });
});
