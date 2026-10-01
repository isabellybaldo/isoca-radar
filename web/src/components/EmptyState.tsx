import { ApiError, SESSION_EXPIRED_TEXT } from '../api/client';
import { Empty } from '../schematics/Empty';

/** The dashboard's empty line: the public view (spec 7b §7.2); the api error handling stays below. */
export function EmptyState({ text }: { text: string }) {
  return <Empty text={text} />;
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
  if (e instanceof TypeError) return SESSION_EXPIRED_TEXT;
  return 'Something broke. Not your fault. Probably.';
}

export function ErrorState({ error }: { error: unknown }) {
  return (
    <p className="quip error" role="alert">
      {errorText(error)}
    </p>
  );
}
