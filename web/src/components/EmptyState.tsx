import { ApiError } from '../api/client';

export function EmptyState({ text }: { text: string }) {
  return <p className="quip empty">{text}</p>;
}

export function Loading({ what = 'data' }: { what?: string }) {
  return (
    <p className="quip loading" role="status">
      <span className="blink" aria-hidden="true">
        ▮
      </span>{' '}
      Loading {what}. The pixels are warming up.
    </p>
  );
}

export function errorText(e: unknown): string {
  if (e instanceof ApiError) return e.detail;
  // A cross-origin redirect to the GitHub login (expired session) surfaces as a TypeError.
  if (e instanceof TypeError) return 'Lost the api. Your session may have expired: reload the page to sign in again.';
  return 'Something broke. Not your fault. Probably.';
}

export function ErrorState({ error }: { error: unknown }) {
  return (
    <p className="quip error" role="alert">
      {errorText(error)}
    </p>
  );
}
