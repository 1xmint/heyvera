import { useCallback, useEffect, useState } from 'react';
import { useAuth } from '../hooks/useAuth';
import {
  BILLING_HISTORY_DEFAULT_LIMIT,
  fetchBillingHistory,
  resolveBillingPath,
  type BillingHistoryEntry,
} from '../api/billing';
import {
  accessStateLabel,
  isPremiumAccess,
  normalizeAccessState,
} from '../utils/accessState';

type BillingStatus = {
  active: boolean;
  plan?: string;
  period_end?: string;
  access_state?: string;
};

async function fetchBillingStatus(token: string): Promise<BillingStatus | null> {
  try {
    const res = await fetch(resolveBillingPath('/api/billing/status'), {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok) return null;
    const raw = (await res.json()) as {
      active?: boolean;
      access_state?: string;
      plan?: { plan_type?: string; billing_period_end?: string } | string;
      period_end?: string;
      plan_info?: { plan_type?: string; billing_period_end?: string };
    };
    const planType =
      typeof raw.plan === 'string'
        ? raw.plan
        : raw.plan?.plan_type ?? raw.plan_info?.plan_type;
    const access = normalizeAccessState(raw.access_state);
    // Prefer server `active` when present; else derive only from known access states.
    const active =
      raw.active === true ||
      (raw.active !== false && isPremiumAccess(access));
    return {
      active,
      plan: planType,
      period_end:
        raw.period_end ??
        (typeof raw.plan === 'object' ? raw.plan?.billing_period_end : undefined) ??
        raw.plan_info?.billing_period_end,
      access_state: access,
    };
  } catch {
    return null;
  }
}

async function openBillingPortal(token: string): Promise<string | null> {
  try {
    const res = await fetch(resolveBillingPath('/api/billing/portal'), {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
    });
    if (!res.ok) return null;
    const data = (await res.json()) as { url?: string; portal_url?: string };
    return data.url ?? data.portal_url ?? null;
  } catch {
    return null;
  }
}

function formatCents(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

function BillingHistorySection({
  getToken,
}: {
  getToken: () => Promise<string | null>;
}) {
  const [items, setItems] = useState<BillingHistoryEntry[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [nextOffset, setNextOffset] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const token = await getToken();
      if (!token || cancelled) {
        if (!cancelled) {
          setError('Sign in required to load billing history.');
          setLoading(false);
        }
        return;
      }
      const page = await fetchBillingHistory(token, {
        limit: BILLING_HISTORY_DEFAULT_LIMIT,
        offset: 0,
      });
      if (cancelled) return;
      if (!page) {
        setError('Billing history unavailable.');
        setLoading(false);
        return;
      }
      setItems(page.items);
      setHasMore(page.hasMore);
      setNextOffset(page.nextOffset);
      setLoading(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [getToken]);

  const loadMore = useCallback(async () => {
    if (loading || !hasMore || nextOffset === null) return;
    setLoading(true);
    setError(null);
    const token = await getToken();
    if (!token) {
      setError('Sign in required to load more billing history.');
      setLoading(false);
      return;
    }
    const page = await fetchBillingHistory(token, {
      limit: BILLING_HISTORY_DEFAULT_LIMIT,
      offset: nextOffset,
    });
    if (!page) {
      setError('Could not load more billing history.');
      setLoading(false);
      return;
    }
    setItems((current) => [...current, ...page.items]);
    setHasMore(page.hasMore);
    setNextOffset(page.nextOffset);
    setLoading(false);
  }, [getToken, hasMore, loading, nextOffset]);

  return (
    <section className="mb-8 rounded-2xl border border-[var(--border-primary)] bg-[var(--bg-elevated)] p-6">
      <h2 className="text-[16px] font-bold text-[var(--text-primary)]">Billing history</h2>
      <p className="mt-1 text-[13px] text-[var(--text-secondary)]">
        Existing subscription events remain available while new checkout is paused.
      </p>

      {items.length > 0 ? (
        <ul className="mt-4 space-y-2 border-t border-[var(--border-primary)] pt-4">
          {items.map((entry, index) => (
            <li
              key={`${entry.date}-${entry.description}-${index}`}
              className="flex items-start justify-between gap-3 text-[13px]"
            >
              <div>
                <div className="text-[var(--text-primary)]">{entry.description}</div>
                <div className="text-[var(--text-secondary)]">
                  {entry.date
                    ? (() => {
                        const date = new Date(entry.date);
                        return Number.isNaN(date.getTime())
                          ? entry.date
                          : date.toLocaleDateString();
                      })()
                    : '—'}
                  {' · '}
                  {entry.status}
                </div>
              </div>
              <div className="shrink-0 font-medium text-[var(--text-primary)]">
                {entry.amount_cents ? formatCents(entry.amount_cents) : '—'}
              </div>
            </li>
          ))}
        </ul>
      ) : !loading ? (
        <p className="mt-4 text-[13px] text-[var(--text-secondary)]">
          {error ?? 'No billing history events yet.'}
        </p>
      ) : null}

      {loading ? (
        <p className="mt-4 text-[13px] text-[var(--text-secondary)]" role="status">
          Loading billing history…
        </p>
      ) : null}
      {hasMore && !loading ? (
        <button
          type="button"
          onClick={() => void loadMore()}
          className="mt-4 w-full rounded-full border border-[var(--border-primary)] py-2 text-[13px] font-semibold text-[var(--text-primary)] transition-opacity hover:opacity-80"
        >
          Load more
        </button>
      ) : null}
    </section>
  );
}

function PremiumMemberView({
  status,
  onManage,
  managing,
}: {
  status: BillingStatus;
  onManage: () => void;
  managing: boolean;
}) {
  const access = normalizeAccessState(status.access_state);
  return (
    <div className="rounded-2xl border border-[var(--accent)] bg-[color-mix(in_srgb,var(--accent)_8%,transparent)] p-6 mb-8">
      <div className="flex items-center gap-3 mb-3">
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="var(--accent)"
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="flex-shrink-0"
        >
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
        </svg>
        <h2 className="text-[18px] font-bold text-[var(--accent)]">
          {access === 'trial_active' ? "You're on a Premium trial" : "You're a Premium member"}
        </h2>
      </div>
      <p className="text-[15px] text-[var(--text-secondary)] mb-5">
        {status.plan ? (
          <>
            You're on the <strong className="text-[var(--text-primary)]">{status.plan}</strong> plan
            {' · '}
            <span className="text-[var(--text-primary)]">{accessStateLabel(access)}</span>.
          </>
        ) : (
          <>
            Access: <strong className="text-[var(--text-primary)]">{accessStateLabel(access)}</strong>.
          </>
        )}
        {status.period_end ? (
          <> Renews on {new Date(status.period_end).toLocaleDateString()}.</>
        ) : null}
      </p>
      <button
        type="button"
        disabled={managing}
        onClick={onManage}
        className="rounded-full border border-[var(--accent)] px-5 py-2.5 text-[15px] font-bold text-[var(--accent)] transition-opacity hover:opacity-80 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {managing ? 'Opening portal...' : 'Manage subscription'}
      </button>
    </div>
  );
}

export function PremiumPage() {
  const { isSignedIn, getToken } = useAuth();

  const [billingStatus, setBillingStatus] = useState<BillingStatus | null>(null);
  const [loadingStatus, setLoadingStatus] = useState(false);
  const [managing, setManaging] = useState(false);
  const [portalError, setPortalError] = useState<string | null>(null);

  useEffect(() => {
    if (!isSignedIn) {
      setBillingStatus(null);
      return;
    }

    setLoadingStatus(true);
    void (async () => {
      try {
        const token = await getToken();
        if (!token) return;
        setBillingStatus(await fetchBillingStatus(token));
      } finally {
        setLoadingStatus(false);
      }
    })();
  }, [isSignedIn, getToken]);

  const handleManage = async () => {
    setManaging(true);
    setPortalError(null);
    try {
      const token = await getToken();
      if (!token) {
        setPortalError('You must be signed in to manage your subscription.');
        return;
      }
      const portalUrl = await openBillingPortal(token);
      if (portalUrl) {
        window.location.href = portalUrl;
      } else {
        setPortalError('Unable to open the billing portal. Please try again.');
      }
    } catch {
      setPortalError('Unable to open the billing portal. Please try again.');
    } finally {
      setManaging(false);
    }
  };

  // Never invent premium — only server active flag or known access_state.
  const isPremium =
    billingStatus?.active === true ||
    isPremiumAccess(billingStatus?.access_state);

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <div className="sticky top-[var(--top-bar-height)] z-10 border-b border-[var(--border-primary)] bg-[color-mix(in_srgb,var(--bg-primary)_80%,transparent)] px-4 py-3 backdrop-blur-md">
        <h1 className="text-[20px] font-bold text-[var(--text-primary)]">HeyVera Premium</h1>
      </div>

      <div className="max-w-2xl mx-auto px-4 py-8">
        <p className="mb-4 text-[15px] leading-relaxed text-[var(--text-secondary)]">
          Existing Premium members can review their subscription and open the billing portal.
          New subscriptions and Pulse draft creation are paused while Socials pricing and entitlements are finalized.
        </p>

        <div className="mb-8 rounded-xl border border-[var(--border-primary)] bg-[var(--bg-elevated)] px-4 py-3 text-[14px] text-[var(--text-secondary)]">
          <strong className="text-[var(--text-primary)]">Honest status.</strong>{' '}
          No checkout session will be created, and Socials will not use Cortex credits during this pause.
        </div>

        {loadingStatus ? (
          <div
            className="mb-8 rounded-2xl border border-[var(--border-primary)] bg-[var(--bg-elevated)] p-6 text-center text-[15px] text-[var(--text-secondary)]"
            role="status"
            aria-live="polite"
          >
            Checking subscription status...
          </div>
        ) : null}

        {portalError ? (
          <div
            className="mb-6 rounded-xl border border-[var(--color-danger)] bg-[color-mix(in_srgb,var(--color-danger)_8%,transparent)] px-4 py-3 text-[14px] text-[var(--color-danger)]"
            role="alert"
          >
            {portalError}
          </div>
        ) : null}

        {!loadingStatus && isPremium ? (
          <PremiumMemberView
            status={
              billingStatus ?? {
                active: true,
                access_state: 'active',
              }
            }
            onManage={() => void handleManage()}
            managing={managing}
          />
        ) : null}

        {isSignedIn ? (
          <BillingHistorySection getToken={getToken} />
        ) : null}

        {!loadingStatus && !isPremium ? (
          <section className="rounded-2xl border border-[var(--border-primary)] bg-[var(--bg-elevated)] p-6">
            <h2 className="text-[17px] font-bold text-[var(--text-primary)]">New subscriptions paused</h2>
            <p className="mt-2 text-[14px] leading-relaxed text-[var(--text-secondary)]">
              Self-serve checkout is unavailable until Socials has its own approved commercial model.
              Existing members can sign in to manage their subscription; no Cortex balance is used here.
            </p>
            <button
              type="button"
              disabled
              className="mt-5 w-full rounded-full bg-[var(--accent)] py-3 text-[15px] font-bold text-[var(--bg-primary)] opacity-50 cursor-not-allowed"
            >
              Checkout unavailable
            </button>
          </section>
        ) : null}
      </div>
    </div>
  );
}

export default PremiumPage;
