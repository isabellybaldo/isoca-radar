import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { checkHonesty } from '../../explain/honesty';
import { CROSSED } from '../../schematics/MirrorBooks';
import { describeWasm, loadTestEngine } from '../../wasm/testEngine';
import { ENGINE_FNS, type Engine, type EngineFn } from '../../wasm/types';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, TAKE } from './copy';
import { CH1, CH2, CH3, CH3_LABELS, CH5, CH6 } from './presets';
import { Story } from './Story';

const sec = (c: HTMLElement, id: string) => c.querySelector(`section#${id}`) as HTMLElement;
const switchOf = (s: HTMLElement) => within(s).getAllByRole('button').filter((b) => b.hasAttribute('aria-pressed'));
const second = (s: HTMLElement) => fireEvent.click(switchOf(s)[1]);
const take = (s: HTMLElement) => s.querySelector('.m-ch-take')?.textContent ?? null;

describe('Story without the engine (spec §3.2)', () => {
  it('reads as prose: seven headed chapters with their leads, no switch, no drawing, no takeaway except chapter 7', () => {
    const { container } = render(<Story engine={null} strip={null} ladder={null} ladderDrawn={false} />);
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(CHAPTERS.map((c) => `${c.n} ${c.question}`));
    for (const c of CHAPTERS) expect(within(sec(container, c.id)).getByText(c.lead)).toBeInTheDocument();
    expect(screen.queryByRole('group')).toBeNull();
    expect(container.querySelectorAll('figure, svg')).toHaveLength(0);
    expect([...container.querySelectorAll('.m-ch-take')].map((p) => p.textContent)).toEqual([TAKE.ch7]);
    expect(screen.queryByText(CH3_COUNT)).toBeNull();
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
  });

  it('the live parts do not wait for the engine: the count line, the strip and the ladder still show, with no switch', () => {
    const { container } = render(<Story engine={null} strip={<p>STRIP</p>} ladder={<p>LADDER</p>} ladderDrawn />);
    const [, , c3] = CHAPTERS;
    expect([...sec(container, 's3').children].map((k) => k.textContent)).toEqual([`${c3.n} ${c3.question}`, c3.lead, CH3_COUNT, 'STRIP']);
    expect([...sec(container, 's7').children].slice(2).map((k) => k.textContent)).toEqual(['LADDER', CH7_EXTRAS, TAKE.ch7]);
    expect(screen.queryByRole('group')).toBeNull();
  });
});

describeWasm('Story on the real engine (spec §3.1, §5.1)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });
  const draw = () => render(<Story engine={engine} strip={<p>STRIP</p>} ladder={<p>LADDER</p>} ladderDrawn />).container;

  it('chapters 1 to 6 each have their heading and lead, a named two-option switch on its first option, a drawing and a takeaway; chapter 7 has no switch', () => {
    const c = draw();
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(CHAPTERS.map((ch) => `${ch.n} ${ch.question}`));
    for (const ch of CHAPTERS) expect(within(sec(c, ch.id)).getByText(ch.lead), ch.id).toBeInTheDocument();
    for (const ch of CHAPTERS.slice(0, 6)) {
      const s = sec(c, ch.id);
      expect(within(s).getByRole('group', { name: ch.switchLabel }), ch.id).toBeInTheDocument();
      expect(switchOf(s).map((b) => b.getAttribute('aria-pressed')), ch.id).toEqual(['true', 'false']);
      if (ch.options !== null) expect(switchOf(s).map((b) => b.textContent), ch.id).toEqual([...ch.options]);
      expect(s.querySelector('figure, svg'), ch.id).not.toBeNull();
      expect(take(s), ch.id).not.toBeNull();
      second(s);
      expect(switchOf(s).map((b) => b.getAttribute('aria-pressed')), ch.id).toEqual(['false', 'true']);
      expect(within(s).getByText(ch.lead), ch.id).toBeInTheDocument();
    }
    expect(within(sec(c, 's7')).queryByRole('group')).toBeNull();
  });

  it('chapter 1: no D1 headline; healthy says two views; crossed shows the alert once and the feed line', () => {
    const s = sec(draw(), 's1');
    expect(s.querySelector('.mb-big')).toBeNull();
    expect(s.querySelector('svg.mb-svg')).not.toBeNull();
    expect(within(s).queryByText(CROSSED)).toBeNull();
    expect(take(s)).toBe(TAKE.ch1(false));
    second(s);
    expect(s.querySelector('.mb-big')).toBeNull();
    expect(within(s).getAllByText(CROSSED)).toHaveLength(1);
    expect(take(s)).toBe(TAKE.ch1(true));
  });

  it('chapter 2: listed says risk-free; not listed shows a ✗ column and says so', () => {
    const s = sec(draw(), 's2');
    expect(take(s)).toBe(TAKE.ch2(false));
    expect(s.textContent).not.toContain('✗');
    second(s);
    expect(s.querySelector('th.broken')).toHaveTextContent('✗');
    expect(take(s)).toBe(TAKE.ch2(true));
  });

  it('chapter 3: the labels are the tick sums; the prices are stacked against the dollar line: no gap at 1.00, a gap and the engine\'s framed edge in the second state; then the count line and the strip', () => {
    const s = sec(draw(), 's3');
    const asks = (v: 0 | 1) => CH3[v].legs.map((l) => l.asks[0].p);
    const stack = () => ({
      segs: s.querySelectorAll('svg.ps-svg rect.ps-leg').length,
      gap: s.querySelectorAll('svg.ps-svg rect.ps-gap').length,
      over: s.querySelectorAll('svg.ps-svg rect.ps-over').length,
      labels: [...s.querySelectorAll('td.ps-leg-l')].map((td) => td.textContent),
    });
    expect(switchOf(s).map((b) => b.textContent)).toEqual([...CH3_LABELS]);
    expect(take(s)).toBe(TAKE.ch3(false));
    expect(s.textContent).not.toContain('$');
    expect(stack()).toEqual({ segs: 3, gap: 0, over: 0, labels: asks(0).map((p, i) => `${'ABC'[i]}${p}`) });
    expect(within(s).queryByText('the gap')).toBeNull();
    // the decision tree is the playground's: here only where it ends
    expect(s.querySelectorAll('.cl-path, .cl-node')).toHaveLength(0);
    expect(s.querySelector('.cl-end')).toHaveTextContent('nothing');
    // the drawing, then the engine's answer, between the switch and the takeaway
    expect([...s.children].map((k) => k.className)).toEqual(['', 'm-ch-lead', 'seg', 'pstack', 'ctree', 'm-ch-take', 'm-ch-lead', '']);
    second(s);
    expect(take(s)).toBe(TAKE.ch3(true));
    expect(stack()).toEqual({ segs: 3, gap: 1, over: 0, labels: asks(1).map((p, i) => `${'ABC'[i]}${p}`) });
    expect(within(s).getByText('the gap')).toBeInTheDocument();
    expect(s.querySelectorAll('.cl-path, .cl-node')).toHaveLength(0);
    expect(s.querySelector('.pstack')?.textContent).not.toContain('$'); // the amount is the engine's, in its own framed line
    expect(s.querySelector('.cl-net')?.textContent).toMatch(/\$\d[\d.,]* on the table/);
    const order = [...s.children].map((k) => k.textContent);
    expect(order.indexOf(CH3_COUNT)).toBeGreaterThan(order.indexOf(TAKE.ch3(true)));
    expect(order.indexOf('STRIP')).toBe(order.indexOf(CH3_COUNT) + 1);
  });

  it('chapter 4: the thin book changes the drawing', () => {
    const s = sec(draw(), 's4');
    const before = s.querySelector('svg')?.outerHTML;
    expect(before).toBeDefined();
    expect(take(s)).toBe(TAKE.ch4);
    second(s);
    expect(s.querySelector('svg')?.outerHTML).not.toBe(before);
    expect(take(s)).toBe(TAKE.ch4);
  });

  it('chapter 5: caught, then missed, each with the matching line', () => {
    const s = sec(draw(), 's5');
    expect(within(s).getByText('caught')).toHaveClass('m-verdict');
    expect(take(s)).toBe(TAKE.ch5(true));
    second(s);
    expect(within(s).getByText('missed')).toHaveClass('m-verdict');
    expect(take(s)).toBe(TAKE.ch5(false));
  });

  it('chapter 6: filled, then caught, each with the matching line, and the drawing changes', () => {
    const s = sec(draw(), 's6');
    const before = s.querySelector('figure')?.outerHTML;
    expect(before).toBeDefined();
    expect(take(s)).toBe(TAKE.ch6('filled'));
    second(s);
    expect(s.querySelector('figure')?.outerHTML).not.toBe(before);
    expect(take(s)).toBe(TAKE.ch6('caught'));
  });

  it('chapter 7: the ladder, the extras line only when the ladder is drawn, and the fixed line', () => {
    const s = sec(draw(), 's7');
    expect(within(s).getByText('LADDER')).toBeInTheDocument();
    expect(within(s).getByText(CH7_EXTRAS)).toHaveClass('dim');
    expect(take(s)).toBe(TAKE.ch7);
    cleanup(); // two stories in one document would share their ids
    const { container } = render(<Story engine={engine} strip={null} ladder={<p>unavailable</p>} ladderDrawn={false} />);
    expect(within(sec(container, 's7')).getByText('unavailable')).toBeInTheDocument();
    expect(within(sec(container, 's7')).queryByText(CH7_EXTRAS)).toBeNull();
    expect(take(sec(container, 's7'))).toBe(TAKE.ch7);
    expect(within(sec(container, 's3')).queryByText(CH3_COUNT)).toBeNull();
  });

  it('spec §2 in both states: every dollar figure is framed in its row or its caption, and no banned word is shown', () => {
    const c = draw();
    const FRAME = /\bon paper\b|\bon the table\b/;
    /** The page test's rule (MathPage.test.tsx, R11): the row around the figure, or the caption of its figure or table. */
    const scan = () => {
      const bare: string[] = [];
      let figures = 0;
      const walker = document.createTreeWalker(c, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
        expect(checkHonesty(n.textContent ?? ''), n.textContent ?? '').toEqual([]);
        if (!/\$\d/.test(n.textContent ?? '')) continue;
        figures += 1;
        const row = n.parentElement?.closest('li, p, tr, figcaption, caption, dl > div') ?? null;
        const fig = n.parentElement?.closest('figure, table') ?? null;
        const captions = fig === null ? [] : [...fig.children].filter((k) => k.matches('figcaption, caption'));
        if (![row, ...captions].some((el) => FRAME.test(el?.textContent ?? ''))) bare.push((row ?? n).textContent ?? '');
      }
      expect(bare).toEqual([]);
      return figures;
    };
    expect(scan()).toBeGreaterThan(0);
    for (const ch of CHAPTERS.slice(0, 6)) second(sec(c, ch.id));
    expect(scan()).toBeGreaterThan(0);
    expect(c.querySelectorAll('a, nav, [style], svg text')).toHaveLength(0);
  });

  it('asks the engine once per chapter on first paint (six calls, one per export), not again when its parent draws again, and once for a switch', () => {
    const calls: EngineFn[] = [];
    const counting = Object.fromEntries(ENGINE_FNS.map((fn) => [fn, (i: string) => (calls.push(fn), engine[fn](i))])) as Engine;
    const { container, rerender } = render(<Story engine={counting} strip={<p>STRIP</p>} ladder={<p>LADDER</p>} ladderDrawn />);
    expect([...calls].sort()).toEqual(['basket_json', 'classify_json', 'depth_walk_json', 'episode_json', 'mirror_json', 'payoff_json']);
    // the page draws again whenever the snapshot or its clock moves: new live parts, the same engine
    rerender(<Story engine={counting} strip={<p>STRIP again</p>} ladder={<p>LADDER again</p>} ladderDrawn />);
    rerender(<Story engine={counting} strip={null} ladder={<p>unavailable</p>} ladderDrawn={false} />);
    expect(within(sec(container, 's7')).getByText('unavailable')).toBeInTheDocument(); // it did draw again
    expect(calls).toHaveLength(6);
    second(sec(container, 's4'));
    expect(calls.slice(6)).toEqual(['depth_walk_json']);
  });

  it('review focus 3: switching one chapter moves no other, for each of the six', () => {
    const c = draw();
    const ids = CHAPTERS.map((ch) => ch.id);
    for (const id of ids.slice(0, 6)) {
      const others = () => ids.filter((x) => x !== id).map((x) => sec(c, x).innerHTML);
      const before = others();
      const own = sec(c, id).innerHTML;
      second(sec(c, id));
      expect(sec(c, id).innerHTML, id).not.toBe(own);
      expect(others(), id).toEqual(before);
    }
  });

  it('chapter 3 with an engine error: the engine\'s sentence, and no price stack', () => {
    const broken: Engine = { ...engine, classify_json: () => JSON.stringify({ error: 'the engine said no' }) };
    const { container } = render(<Story engine={broken} strip={null} ladder={null} ladderDrawn={false} />);
    const s = sec(container, 's3');
    expect(within(s).getByText('the engine said no')).toHaveClass('error');
    expect(s.querySelector('.pstack, svg, .ctree')).toBeNull();
    expect(take(s)).toBeNull();
    expect(switchOf(s)).toHaveLength(2);
  });

  it('the takeaway follows the answer of the engine, not the switch: an engine that always answers for the second state', () => {
    // each export ignores what it is asked and answers for its chapter's second preset, so the switch (on its
    // first option) and the engine's flag disagree
    const fixed = (fn: EngineFn, input: unknown) => () => engine[fn](JSON.stringify(input));
    const swapped: Engine = {
      ...engine,
      mirror_json: fixed('mirror_json', CH1[1]),
      payoff_json: fixed('payoff_json', CH2[1]),
      classify_json: fixed('classify_json', CH3[1]),
      episode_json: fixed('episode_json', CH5[1]),
      basket_json: fixed('basket_json', CH6[1]),
    };
    const { container } = render(<Story engine={swapped} strip={null} ladder={null} ladderDrawn={false} />);
    const want: [string, string][] = [
      ['s1', TAKE.ch1(true)], ['s2', TAKE.ch2(true)], ['s3', TAKE.ch3(true)], ['s5', TAKE.ch5(false)], ['s6', TAKE.ch6('caught')],
    ];
    for (const [id, line] of want) {
      const s = sec(container, id);
      expect(switchOf(s).map((b) => b.getAttribute('aria-pressed')), id).toEqual(['true', 'false']);
      expect(take(s), id).toBe(line);
      second(s); // the switch moves, the engine's answer does not: the same line stays
      expect(switchOf(s).map((b) => b.getAttribute('aria-pressed')), id).toEqual(['false', 'true']);
      expect(take(s), id).toBe(line);
    }
  });

  it('review focus 1: an engine that answers with an error shows its sentence and no takeaway', () => {
    const broken: Engine = { ...engine, depth_walk_json: () => JSON.stringify({ error: 'levels must be cheapest first' }) };
    const { container } = render(<Story engine={broken} strip={null} ladder={null} ladderDrawn={false} />);
    const s = sec(container, 's4');
    expect(within(s).getByRole('status')).toHaveTextContent('levels must be cheapest first');
    expect(s.querySelector('figure, svg')).toBeNull();
    expect(take(s)).toBeNull();
    expect(take(sec(container, 's1'))).toBe(TAKE.ch1(false));
  });
});
