// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { FIXTURES, fixtureCase } from '../wasm/testEngine';
import { CRYPTO_FEE, against, basketInput, drain } from './basket';
import { F_BOOKS, F_BURST, F_START } from './defaults';

/** What JSON.stringify would send (undefined fields gone), to compare with a fixture's parsed input. */
const wire = (x: unknown): unknown => JSON.parse(JSON.stringify(x));

describe('section f input (spec 7b §6.2 f, 7b-A basket_json)', () => {
  it('drains the t_x depth in whole shares and drops an emptied level', () => {
    expect(drain(F_BOOKS[0].bids, 50)).toEqual([{ p: '0.40', s: '50' }]);
    expect(drain(F_BOOKS[0].bids, 0)).toEqual([]);
    expect(drain(F_BOOKS[0].bids, 100)).toEqual(F_BOOKS[0].bids);
    expect(drain([{ p: '0.40', s: '7' }], 50)).toEqual([{ p: '0.40', s: '3' }]);
  });

  it('moves the unwind book against a NO holder: the YES asks rise, the bids stay', () => {
    const b = against(F_BOOKS[1], 5);
    expect(b.asks).toEqual([{ p: '0.41', s: '100' }]);
    expect(b.bids).toEqual(F_BOOKS[1].bids);
  });

  it('keeps the unwind book on the 0.01–0.99 grid at the largest move', () => {
    const b = against({ bids: [{ p: '0.90', s: '10' }], asks: [{ p: '0.95', s: '10' }] }, 20);
    expect(b.asks).toEqual([{ p: '0.99', s: '10' }]);
  });

  it('keeps a two-level side valid when the cap would pile both on 0.99', () => {
    const two = { bids: [], asks: [{ p: '0.98', s: '10' }, { p: '0.99', s: '20' }] };
    expect(against(two, 5).asks).toEqual([{ p: '0.99', s: '10' }]);
    expect(against(two, 1).asks).toEqual([{ p: '0.99', s: '10' }]);
    expect(against(two, 0).asks).toEqual(two.asks);
  });

  it('drains in integer maths only, and leaves a size that is not a plain integer alone', () => {
    expect(drain([{ p: '0.40', s: '99999999999999999999' }], 50)).toEqual([{ p: '0.40', s: '49999999999999999999' }]);
    expect(drain([{ p: '0.40', s: '12.5' }], 50)).toEqual([{ p: '0.40', s: '12.5' }]);
  });

  it('sends a fee on every leg, and no min_order or convert_fee_bips (the engine defaults "5" and "0")', () => {
    const start = basketInput(F_BOOKS, F_START);
    expect(Object.keys(start).sort()).toEqual(['kind', 'legs', 'q_request']);
    expect(start.legs.map((l) => l.fee)).toEqual([null, null, null]);
    expect(basketInput(F_BOOKS, { ...F_START, fees: true }).legs.map((l) => l.fee)).toEqual([CRYPTO_FEE, CRYPTO_FEE, CRYPTO_FEE]);
  });
});

describe.skipIf(!existsSync(`${FIXTURES}/basket_json.json`))('section f starts from 7b-A fixtures (review focus 4)', () => {
  it('the start is fixture no-all-fill, the usual burst is no-one-leg-empty-burst', () => {
    expect(wire(basketInput(F_BOOKS, F_START))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-all-fill').input));
    expect(wire(basketInput(F_BOOKS, F_BURST))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-one-leg-empty-burst').input));
  });

  it('leg C at 20 % is no-one-leg-partial, fees on is no-fees-on', () => {
    expect(wire(basketInput(F_BOOKS, { ...F_START, depth: [100, 100, 20] }))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-one-leg-partial').input));
    expect(wire(basketInput(F_BOOKS, { ...F_START, fees: true }))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-fees-on').input));
  });
});
