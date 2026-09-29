// Spec 7a §9.4 (09-27 §7.2): the panel's text never claims real money. Fix the copy, never widen the rule.
const BANNED = /\b(earned|earn|made|income|profitable)\b/gi;
/** `profit` is allowed only inside the existing help.ts phrases. */
const PROFIT_OK = /profit @ \$100|net profit at the peak|profit (?:after|before) taker fees/gi;
const MONEY = /\$\d/;
const LABEL = /paper|on the table|captured at \d+ ms/i;

/** The offending words, lower case, in text order; [] means honest. */
export function checkHonesty(text: string): string[] {
  const hits = [...text.matchAll(BANNED)].map((m) => m[0].toLowerCase());
  const bare = [...text.replace(PROFIT_OK, '').matchAll(/\bprofits?\b/gi)].map((m) => m[0].toLowerCase());
  return [...hits, ...bare];
}

/** Every block (li, p, table, figure, dd) that shows a "$<digit>" figure without saying it is paper, on
 *  the table, or captured at N ms. Returns their texts; [] means every figure is labelled. */
export function unlabelledMoney(root: HTMLElement): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
    if (!MONEY.test(n.textContent ?? '')) continue;
    const el = n.parentElement;
    const found = el?.closest('li, p, table, figure, dd') ?? el?.closest('div, section, td, th') ?? null;
    // Never the whole root: a figure outside every block would borrow a label from anywhere in the panel.
    const block = found !== null && found !== root && root.contains(found) ? found : null;
    const text = (block ?? el ?? root).textContent?.replace(/\s+/g, ' ').trim() ?? '';
    if ((block === null || !LABEL.test(text)) && !out.includes(text)) out.push(text);
  }
  return out;
}
