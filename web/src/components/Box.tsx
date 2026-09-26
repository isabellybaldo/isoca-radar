import type { ReactNode } from 'react';

export function Box({ title, className, children }: { title?: ReactNode; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `box ${className}` : 'box'}>
      {title !== undefined && <h2>{title}</h2>}
      {children}
    </section>
  );
}
