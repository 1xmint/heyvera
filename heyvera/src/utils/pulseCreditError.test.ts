import { describe, expect, it } from 'vitest';
import {
  isInsufficientCreditsError,
  pulseCreditErrorMessage,
} from './pulseCreditError';

describe('pulseCreditErrorMessage', () => {
  it('surfaces insufficient credits from API message', () => {
    const msg = pulseCreditErrorMessage(
      new Error('insufficient credits: need 1.00, have 0.00'),
    );
    expect(msg).toContain('insufficient credits');
    expect(msg).toContain('need 1.00');
  });

  it('uses 402 status with fallback when message empty', () => {
    const msg = pulseCreditErrorMessage('', 402);
    expect(msg.toLowerCase()).toContain('insufficient credits');
  });

  it('does not treat generic 403 as credits without keywords', () => {
    expect(isInsufficientCreditsError('forbidden', 403)).toBe(false);
    expect(isInsufficientCreditsError('insufficient credits: need 1', 403)).toBe(
      true,
    );
  });

  it('passes through non-credit errors', () => {
    expect(pulseCreditErrorMessage(new Error('Create a profile first'))).toBe(
      'Create a profile first',
    );
  });

  it('surfaces the stable Socials billing pause without claiming drafts are free', () => {
    const msg = pulseCreditErrorMessage(
      '',
      503,
      'SOCIALS_BILLING_UNAVAILABLE',
    );
    expect(msg).toContain('temporarily unavailable');
    expect(msg).toContain('Socials billing');
    expect(msg.toLowerCase()).not.toContain('free');
  });
});
