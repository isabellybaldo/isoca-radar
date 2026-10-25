import { RunCost } from '../schematics/RunCost';

/** Frozen-site plan, task 4: the landing. Copy is the plan's, verbatim; the cost section is `#cost` because
 *  /math's last line links to /#cost (ruling R1). */
export function Landing() {
  return (
    <>
      <div className="pf-intro">
        <h1 className="pf-title">isoca-radar</h1>
        <p className="pf-lead">
          A read-only lab for Polymarket arbitrage, run from 2026-09-26 to 2026-10-19. It watched the order books, found
          baskets whose prices added up to less than a dollar, and paper-traded them at six latencies. Paper only: it
          never placed an order.
        </p>
      </div>
      <div className="grid2">
        <section className="box pf-card">
          <h2><a href="/math">The math</a></h2>
          <p>
            Seven chapters that run the scanner’s own Rust code in your browser: how an arbitrage is found, sized and
            timed, and what goes wrong between seeing it and holding it.
          </p>
        </section>
        <section className="box pf-card">
          <h2><a href="/sweet">Sweet spots</a></h2>
          <p>Where the edge survived fees and money tied up, over the last three days of paper trading (110 trades).</p>
        </section>
      </div>
      <section className="box hero pf-cost" id="cost" aria-labelledby="cost-h">
        <h2 id="cost-h">What it took to run live</h2>
        <RunCost />
        <p className="quip">
          It was switched off on 2026-10-19, when the host started limiting its CPU. The code is still in the repo; this
          page is what is left running.
        </p>
      </section>
    </>
  );
}
