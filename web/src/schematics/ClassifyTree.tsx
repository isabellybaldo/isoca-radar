import { Money } from '../components/Money';
import type { ClassifyModel, ClassifyNode } from '../wasm/types';

/** The classify path nodes, in the order arb_core::classify::classify runs them, with a label each. */
export const NODES: [ClassifyNode, string][] = [
  ['decided', 'is the event decided?'],
  ['no_basket_tried', 'NO basket over the legs with a YES bid'],
  ['no_basket_few_legs', 'fewer than two legs have a YES bid'],
  ['no_basket_crossed', 'a crossed book: our state is wrong'],
  ['no_basket_not_positive', 'Σ YES bids − 1 is not positive'],
  ['no_basket_positive', 'the NO basket pays'],
  ['yes_basket_tried', 'YES basket over every live leg'],
  ['yes_basket_no_live', 'no live leg'],
  ['yes_basket_crossed', 'a crossed book: our state is wrong'],
  ['yes_basket_missing_ask', 'a live leg has no ask'],
  ['yes_basket_not_positive', '1 − Σ asks is not positive'],
  ['yes_basket_positive', 'the YES basket pays'],
  ['complete', 'every outcome is in the set'],
  ['incomplete', 'an outcome is not in the set'],
];

const KIND: Record<string, string> = {
  yes_basket: 'YES basket', yes_basket_incomplete: 'YES basket, incomplete set', no_basket: 'NO basket', implication: 'implication',
};
const REALIZATION: Record<string, string> = { settlement: 'held to settlement', converter: 'converted', not_arbitrage: 'not arbitrage' };

/** Where the path ends: the kind and how it is realized, or "nothing". */
export function endText(m: ClassifyModel): string {
  if (m.kind === null) return 'nothing';
  const k = KIND[m.kind] ?? m.kind;
  return m.realization === null ? k : `${k} · ${REALIZATION[m.realization] ?? m.realization}`;
}

/**
 * Parent §5.7 (c): every test classify can run, top to bottom, the ones it ran lit. HTML only (no SVG to
 * stretch). A node the page does not know is shown under its own name, never dropped (contract drift shows).
 */
export function ClassifyTree({ model }: { model: ClassifyModel }) {
  const known = new Set<string>(NODES.map(([id]) => id));
  const extra = [...new Set(model.path.filter((n) => !known.has(n)))].map((n): [string, string] => [n, n]);
  return (
    <figure className="ctree" aria-label="classify one update">
      <ol className="cl-path" aria-label="the tests classify runs">
        {[...(NODES as [string, string][]), ...extra].map(([id, text]) => {
          const on = model.path.includes(id);
          return (
            <li key={id} className={on ? 'cl-node lit' : 'cl-node'} data-node={id} aria-current={on ? 'step' : undefined}>
              <span className="cl-dot" aria-hidden="true" />
              <span>{text}</span>
            </li>
          );
        })}
      </ol>
      <p className="cl-end">{endText(model)}</p>
      {model.net !== null && (
        <p className="cl-net">
          net Π <Money value={model.net} signed cents /> on the table · gross <Money value={model.gross} signed cents />
          {model.q_star !== null && ` · Q* ${model.q_star}`}
        </p>
      )}
    </figure>
  );
}
