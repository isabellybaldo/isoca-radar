export type TagTone = 'plain' | 'no' | 'yes' | 'flag';

export function Tag({ children, tone = 'plain', title }: { children: string; tone?: TagTone; title?: string }) {
  return (
    <span className={`tag ${tone}`} title={title}>
      {children}
    </span>
  );
}
