// Spec 7a §9.1: open/closed and the last question open, in localStorage. Every access is guarded: a
// private window or blocked storage falls back to the defaults (closed, the hero).
const OPEN = 'explain:open';
const QUESTION = 'explain:q';

function get(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}

function set(k: string, v: string): void {
  try {
    localStorage.setItem(k, v);
  } catch {
    // storage blocked: the panel still works, it just forgets
  }
}

export const readOpen = (): boolean => get(OPEN) === '1';
export const writeOpen = (v: boolean): void => set(OPEN, v ? '1' : '0');
export const readQuestion = (): string | null => get(QUESTION);
export const writeQuestion = (id: string): void => set(QUESTION, id);
