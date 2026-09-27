import type { TagTone } from '../components/Tag';

/** Tags carry text, not colour alone (spec §5). */
export function kindTag(kind: string, long = false): { text: string; tone: TagTone } {
  switch (kind) {
    case 'no_basket':
      return { text: long ? 'NO BASKET' : 'NO', tone: 'no' };
    case 'yes_basket':
      return { text: long ? 'YES BASKET' : 'YES', tone: 'yes' };
    case 'yes_basket_incomplete':
      return { text: long ? 'YES BASKET?' : 'YES?', tone: 'yes' };
    case 'implication':
      return { text: long ? 'IMPLICATION' : 'IMPL', tone: 'plain' };
    default:
      return { text: kind.toUpperCase(), tone: 'plain' };
  }
}

/** A long market id, short enough to tell rows apart: "0x5f2e…9a01" (the full id goes in a title). */
export function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 6)}…${id.slice(-4)}` : id;
}
