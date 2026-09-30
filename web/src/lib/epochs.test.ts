import { describe, expect, it } from 'vitest';
import { EPOCH1_START, accountFixture, epochsFixture } from '../test/fixtures';
import { currentEpochText, earlierEpochs, epochAccounts, epochLine, nextAllowedText } from './epochs';

const S2 = Date.UTC(2026, 8, 30, 8, 0);
const two = epochsFixture({
  epochs: [{ epoch: 1, start_ms: EPOCH1_START, cause: 'policies' }, { epoch: 2, start_ms: S2, cause: 'reset' }],
  current: 2,
});

describe('epochs', () => {
  it('says since when and which epoch, by cause', () => {
    expect(epochLine(epochsFixture())).toBe('since new policies 2026-09-29 19:58 UTC · epoch 1');
    expect(epochLine(two)).toBe('since reset 2026-09-30 08:00 UTC · epoch 2');
    expect(epochLine(epochsFixture({ epochs: [], current: 0 }))).toBe('epoch 0 · no reset yet');
  });

  it('lists every earlier epoch newest first, epoch 0 without a start', () => {
    expect(earlierEpochs(two)).toEqual([
      { epoch: 1, start_ms: EPOCH1_START, end_ms: S2, cause: 'policies' },
      { epoch: 0, start_ms: null, end_ms: EPOCH1_START, cause: 'before epochs' },
    ]);
    expect(earlierEpochs(epochsFixture({ epochs: [], current: 0 }))).toEqual([]);
  });

  it('keeps the P0 accounts at fee 0, ladder order first, then the extras', () => {
    const p0 = (scenario: string, o = {}) => accountFixture({ account_id: `P0-converter/${scenario}`, scenario, ...o });
    const got = epochAccounts([
      p0('sp-idle'), p0('sp-burst'), p0('rung-0'), p0('sp-idle', { account_id: 'P0-converter/sp-idle@10', fee_bips: '10' }),
      accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive' }), p0('colo-idle'), p0('sp-idle-nodelay'),
    ]);
    expect(got.map((a) => a.account_id)).toEqual([
      'P0-converter/rung-0', 'P0-converter/colo-idle', 'P0-converter/sp-idle', 'P0-converter/sp-burst', 'P0-converter/sp-idle-nodelay',
    ]);
  });

  it('writes the reset fold lines', () => {
    expect(currentEpochText(epochsFixture())).toBe('epoch 1, started 2026-09-29 19:58 UTC (policies)');
    expect(currentEpochText(epochsFixture({ epochs: [], current: 0 }))).toBe('epoch 0: no epoch has started yet');
    expect(nextAllowedText(Date.UTC(2026, 9, 1))).toBe('next reset allowed after 2026-10-01 00:00 UTC');
  });
});
