/** Spec 7b §7.2: the plain empty-state line. No api import, so the public bundle can use it; the dashboard's
 *  EmptyState renders it and keeps its ApiError/session handling (errorText, ErrorState) beside it. */
export function Empty({ text }: { text: string }) {
  return <p className="quip empty">{text}</p>;
}
