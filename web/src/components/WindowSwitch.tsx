// F6 (controller ruling): WINS, Win and the window-switch button group live in exactly one place,
// shared by the Papers page and Task 13's EdgeCapture, instead of each declaring its own copy.
export const WINS = [
  ['24h', '24H'],
  ['7d', '7D'],
  ['30d', '30D'],
  ['all', 'ALL'],
] as const;
export type Win = (typeof WINS)[number][0];

export function isWin(v: string | null): v is Win {
  return WINS.some(([w]) => w === v);
}

/** The 24H/7D/30D/ALL window switch (spec §8.2). */
export function WindowSwitch({ win, onChange }: { win: Win; onChange: (w: Win) => void }) {
  return (
    <div className="seg" role="group" aria-label="window">
      {WINS.map(([w, label]) => (
        <button key={w} type="button" className={w === win ? 'on' : undefined} aria-pressed={w === win} onClick={() => onChange(w)}>
          {label}
        </button>
      ))}
    </div>
  );
}
