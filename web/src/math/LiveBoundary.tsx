import { Component, type ReactNode } from 'react';

/**
 * Spec 7b §6.3: the rest of the page never depends on the snapshot. If a live part throws while rendering
 * (a shape parseSnapshot let through and a drawing cannot take), this draws nothing in its place and tells the
 * page, which shows its "live numbers unavailable" line in both places. The story and sections a to f are outside
 * it and keep working.
 */
export class LiveBoundary extends Component<{ onError: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(): void {
    this.props.onError();
  }

  render(): ReactNode {
    return this.state.failed ? null : this.props.children;
  }
}
