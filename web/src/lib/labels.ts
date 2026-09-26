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
