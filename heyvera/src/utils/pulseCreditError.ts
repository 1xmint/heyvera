/**
 * Honest Pulse creation error copy during the Socials billing transition.
 * Never imply that draft creation is free when its commercial policy is unavailable.
 */

/** True when an error message or HTTP status indicates insufficient credits. */
export function isInsufficientCreditsError(
  message: string | null | undefined,
  status?: number | null,
): boolean {
  if (status === 402 || status === 403) {
    // 403 only counts when the body also mentions credits (avoid masking real auth errors).
    if (status === 403) {
      return /insufficient credits|INSUFFICIENT_CREDITS/i.test(message ?? '');
    }
    return true;
  }
  return /insufficient credits|INSUFFICIENT_CREDITS/i.test(message ?? '');
}

/**
 * Map API / thrown errors to user-facing Pulse create copy.
 * Prefers the server error string when present.
 */
export function pulseCreditErrorMessage(
  err: unknown,
  status?: number | null,
  code?: string | null,
): string {
  const raw =
    err instanceof Error
      ? err.message
      : typeof err === 'string'
        ? err
        : err && typeof err === 'object' && 'error' in err
          ? String((err as { error?: unknown }).error ?? '')
          : '';
  const msg = raw.trim();

  if (
    code === 'SOCIALS_BILLING_UNAVAILABLE' ||
    /SOCIALS_BILLING_UNAVAILABLE/i.test(msg) ||
    (status === 503 && /socials billing|commercial model/i.test(msg))
  ) {
    return (
      msg ||
      'Pulse draft creation is temporarily unavailable while Socials billing is being finalized.'
    );
  }

  if (isInsufficientCreditsError(msg, status)) {
    return (
      msg ||
      'Insufficient credits to create a Pulse draft. Check Premium for your balance when metered.'
    );
  }

  return msg || 'Could not create Pulse draft. Try again.';
}
