import { useCallback, useEffect, useRef, useState } from 'react';
import { RefreshCw, Send, Sparkles, Zap } from 'lucide-react';
import { SignInButton } from '@clerk/clerk-react';
import { Link } from 'react-router';
import { useAuth } from '../hooks/useAuth';
import {
  listDrafts,
  approveDraft,
  rejectDraft,
  publishDraft,
  pulseChat,
  getDraftAudit,
  scheduleDraft,
  listSchedules,
  createGoal,
  listGoals,
} from '../api/pulse';
import type {
  PulseAuditEntry,
  PulseChatMode,
  PulseDraft,
  PulseGoal,
  PulseSchedule,
} from '../api/pulse';
import { fetchX402Status, postX402PaidPing } from '../api/social';
import type { X402PaidPingResult, X402Status } from '../api/social';
import { PulseApiError } from '../api/pulse';
import { pulseCreditErrorMessage } from '../utils/pulseCreditError';
import {
  canPulseDraftAction,
  pulseDraftActionLabel,
  pulseTransitionErrorMessage,
} from '../utils/pulseDraftActions';
import {
  PULSE_SCHEDULE_EMPTY_DETAIL,
  PULSE_SCHEDULE_PROCESS_ENDPOINT_HINT,
  PULSE_SCHEDULE_PROCESSOR_NOTE,
  pulseScheduleProcessorStatusLabel,
} from '../utils/pulseScheduleHonesty';
import {
  formatX402PaidPingResult,
  showX402PaidAction,
} from '../utils/x402Enabled';

type Tab = 'drafts' | 'schedule' | 'goals' | 'helper';

const TABS: { id: Tab; label: string }[] = [
  { id: 'drafts', label: 'Drafts' },
  { id: 'schedule', label: 'Schedule' },
  { id: 'goals', label: 'Goals' },
  { id: 'helper', label: 'Draft helper' },
];

type ChatMessage = {
  id: string;
  role: 'user' | 'vera';
  content: string;
  timestamp: number;
  mode?: PulseChatMode;
  toolsUsed?: string[];
};

const VERA_GREETING =
  "Draft helper (beta): I create and manage Pulse drafts via tools. Without server LLM keys I use keyword tools (tools_v1); with keys, tools_v2. Not a general chat model — use Drafts / Schedule / Goals for the full workflow.";

function X402AgentPaymentsCard({
  isSignedIn,
  getToken,
}: {
  isSignedIn: boolean;
  getToken: () => Promise<string | null>;
}) {
  const [status, setStatus] = useState<X402Status | null>(null);
  const [loadState, setLoadState] = useState<'loading' | 'live' | 'error'>('loading');
  const [pingBusy, setPingBusy] = useState(false);
  const [pingResult, setPingResult] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchX402Status()
      .then((s) => {
        if (!cancelled) {
          setStatus(s);
          setLoadState('live');
        }
      })
      .catch(() => {
        if (!cancelled) setLoadState('error');
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const enabled = status?.enabled === true;
  const mode = status?.mode ?? (enabled ? 'shape_only' : 'disabled');
  const label =
    loadState === 'loading'
      ? 'Checking…'
      : loadState === 'error'
        ? 'Unavailable'
        : mode === 'facilitator'
          ? 'Facilitator'
          : mode === 'shape_only'
            ? 'Shape-only'
            : 'Disabled';
  const showPaidAction = showX402PaidAction(status);

  async function runPaidPing() {
    setPingBusy(true);
    setPingResult(null);
    try {
      const token = await getToken();
      if (!token) {
        setPingResult('Sign in required to test the paid action.');
        return;
      }
      const result: X402PaidPingResult = await postX402PaidPing(token, {
        amount: '0.01',
        network: status?.network,
        payload: { x402Version: 1, source: 'ai-page-test-paid-action' },
        idempotencyKey: `ui-ai-${Date.now()}`,
      });
      setPingResult(formatX402PaidPingResult(result));
    } catch (e) {
      const err = e as Error & { body?: X402PaidPingResult };
      if (err.body) {
        setPingResult(formatX402PaidPingResult(err.body));
      } else {
        setPingResult(e instanceof Error ? e.message : 'Paid action failed');
      }
    } finally {
      setPingBusy(false);
    }
  }

  return (
    <div
      className="mx-4 mt-4 rounded-2xl border px-4 py-3"
      style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)' }}
      role="status"
    >
      <div className="flex items-start gap-2">
        <Zap className="mt-0.5 h-4 w-4 shrink-0" style={{ color: 'var(--accent)' }} aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="text-[14px] font-bold">Agent payments (x402)</p>
          <p className="mt-1 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            Status: <span className="font-semibold" style={{ color: 'var(--text-primary)' }}>{label}</span>
            {status?.network ? ` · network: ${status.network}` : ''}
            {status?.mode ? ` · mode: ${status.mode}` : ''}
            {status?.payTo ? ` · payTo: ${status.payTo}` : ''}
            {status?.note ? ` · ${status.note}` : ''}
            . Details in{' '}
            <Link to="/settings" className="underline" style={{ color: 'var(--accent)' }}>
              Settings
            </Link>
            .
          </p>
          {showPaidAction && isSignedIn ? (
            <div className="mt-2 flex flex-col gap-1.5">
              <button
                type="button"
                disabled={pingBusy}
                onClick={() => void runPaidPing()}
                className="self-start rounded-full border px-3 py-1 text-[12px] font-semibold transition-opacity disabled:opacity-50"
                style={{ borderColor: 'var(--accent)', color: 'var(--accent)' }}
              >
                {pingBusy ? 'Testing…' : 'Test paid action'}
              </button>
              {pingResult ? (
                <p className="text-[12px] font-mono" style={{ color: 'var(--text-primary)' }}>
                  {pingResult}
                </p>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

export function AIPage() {
  const { authEnabled, isSignedIn, getToken } = useAuth();
  const [activeTab, setActiveTab] = useState<Tab>('drafts');

  return (
    <div className="min-h-screen" style={{ backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' }}>
      <div className="sticky top-[var(--top-bar-height)] z-10 border-b sticky-header-bg backdrop-blur-md" style={{ borderColor: 'var(--border-primary)' }}>
        <div className="flex items-center gap-2 px-4 py-3">
          <Sparkles className="h-5 w-5" style={{ color: 'var(--accent)' }} />
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="text-[20px] font-bold">Pulse</h1>
              <span
                className="rounded-full border px-2 py-0.5 text-[11px] font-medium"
                style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-secondary)' }}
              >
                Beta — automation
              </span>
            </div>
            <p className="text-[13px] leading-tight" style={{ color: 'var(--text-secondary)' }}>
              Drafts, schedules, and goals for your active Page
            </p>
          </div>
        </div>
        <div className="flex" style={{ borderTop: '1px solid var(--border-primary)' }}>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className="flex-1 py-3 text-[14px] font-medium transition-colors hover-overlay"
              style={{ color: activeTab === tab.id ? 'var(--text-primary)' : 'var(--text-secondary)' }}
            >
              <span className="relative inline-block">
                {tab.label}
                {activeTab === tab.id && (
                  <span
                    className="absolute -bottom-[13px] left-0 right-0 h-[3px] rounded-full"
                    style={{ backgroundColor: 'var(--accent)' }}
                  />
                )}
              </span>
            </button>
          ))}
        </div>
      </div>

      <div
        className="mx-4 mt-4 rounded-xl border px-4 py-3 text-[13px] leading-relaxed"
        style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)', color: 'var(--text-secondary)' }}
        role="status"
      >
        <strong style={{ color: 'var(--text-primary)' }}>New draft creation is paused.</strong>{' '}
        Existing drafts, approvals, publishing, schedules, and goals remain available while Socials billing is finalized.
        Pulse does not use Cortex credits.
      </div>

      <X402AgentPaymentsCard isSignedIn={Boolean(isSignedIn)} getToken={getToken} />

      {activeTab === 'drafts' && (
        <DraftsTab authEnabled={authEnabled} isSignedIn={isSignedIn} getToken={getToken} />
      )}
      {activeTab === 'schedule' && (
        <ScheduleTab authEnabled={authEnabled} isSignedIn={isSignedIn} getToken={getToken} />
      )}
      {activeTab === 'goals' && (
        <GoalsTab authEnabled={authEnabled} isSignedIn={isSignedIn} getToken={getToken} />
      )}
      {activeTab === 'helper' && (
        <ChatTab authEnabled={authEnabled} isSignedIn={isSignedIn} getToken={getToken} />
      )}
    </div>
  );
}

function ChatTab({ authEnabled, isSignedIn, getToken }: { authEnabled: boolean; isSignedIn: boolean; getToken: () => Promise<string | null> }) {
  const [messages, setMessages] = useState<ChatMessage[]>([
    { id: 'greeting', role: 'vera', content: VERA_GREETING, timestamp: Date.now() },
  ]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [lastMode, setLastMode] = useState<PulseChatMode | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || sending) return;

    const userMsg: ChatMessage = { id: `u-${Date.now()}`, role: 'user', content: text, timestamp: Date.now() };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setSending(true);

    try {
      let veraReply: string;
      let mode: PulseChatMode | undefined;
      let toolsUsed: string[] | undefined;

      if (!authEnabled || !isSignedIn) {
        veraReply =
          "Sign in to use Pulse tools. I can create and list drafts on the server once you're authenticated — then approve/publish/schedule from the Drafts tab.";
      } else {
        const token = await getToken();
        if (!token) {
          veraReply = "I couldn't verify your session. Try signing in again.";
        } else {
          const history = messages
            .filter((m) => m.id !== 'greeting')
            .slice(-10)
            .map((m) => ({
              role: m.role === 'vera' ? 'assistant' : 'user',
              content: m.content,
            }));
          const result = await pulseChat(token, text, history);
          veraReply = result.reply;
          mode = result.mode;
          toolsUsed = result.toolsUsed;
          setLastMode(result.mode);
        }
      }

      const veraMsg: ChatMessage = {
        id: `v-${Date.now()}`,
        role: 'vera',
        content: veraReply,
        timestamp: Date.now(),
        mode,
        toolsUsed,
      };
      setMessages((prev) => [...prev, veraMsg]);
    } catch (err) {
      const status = err instanceof PulseApiError ? err.status : null;
      const code = err instanceof PulseApiError ? err.code : null;
      const serverMsg = err instanceof Error ? err.message : '';
      const content =
        status === 402 ||
        code === 'SOCIALS_BILLING_UNAVAILABLE' ||
        /insufficient credits|INSUFFICIENT_CREDITS|SOCIALS_BILLING_UNAVAILABLE/i.test(serverMsg)
          ? pulseCreditErrorMessage(err, status, code)
          : serverMsg ||
            "Couldn't reach Pulse tools. Check your connection and try again.";
      const errMsg: ChatMessage = {
        id: `e-${Date.now()}`,
        role: 'vera',
        content,
        timestamp: Date.now(),
      };
      setMessages((prev) => [...prev, errMsg]);
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="flex flex-col" style={{ height: 'calc(100vh - 160px)' }}>
      {lastMode && (
        <div className="flex items-center gap-2 border-b px-4 py-2" style={{ borderColor: 'var(--border-primary)' }}>
          <span className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>Last response mode</span>
          <span
            className="rounded-full border px-2 py-0.5 text-[11px] font-semibold"
            style={{
              borderColor: lastMode === 'tools_v2' ? 'var(--accent)' : 'var(--border-secondary)',
              color: lastMode === 'tools_v2' ? 'var(--accent)' : 'var(--text-secondary)',
            }}
            title={
              lastMode === 'tools_v2'
                ? 'LLM tool routing (server ANTHROPIC_API_KEY or OPENAI_API_KEY)'
                : 'Keyword tools only (no LLM keys or LLM fallback)'
            }
          >
            {lastMode}
          </span>
        </div>
      )}
      <div className="flex-1 overflow-y-auto px-4 py-4">
        {messages.map((msg) => (
          <div key={msg.id} className={`mb-4 flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
            {msg.role === 'vera' && (
              <div className="mr-2 mt-1 flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full" style={{ backgroundColor: 'var(--accent)' }}>
                <Sparkles className="h-4 w-4" style={{ color: '#000' }} />
              </div>
            )}
            <div
              className="max-w-[80%] rounded-2xl px-4 py-2.5"
              style={{
                backgroundColor: msg.role === 'user' ? 'var(--accent)' : 'var(--bg-elevated)',
                color: msg.role === 'user' ? '#000' : 'var(--text-primary)',
              }}
            >
              <p className="whitespace-pre-wrap text-[15px] leading-relaxed">{msg.content}</p>
              {msg.role === 'vera' && msg.mode && (
                <div className="mt-2 flex flex-wrap items-center gap-1.5">
                  <span
                    className="rounded-full border px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide"
                    style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-tertiary)' }}
                  >
                    {msg.mode}
                  </span>
                  {msg.toolsUsed && msg.toolsUsed.length > 0 && (
                    <span className="text-[10px]" style={{ color: 'var(--text-tertiary)' }}>
                      {msg.toolsUsed.join(', ')}
                    </span>
                  )}
                </div>
              )}
            </div>
          </div>
        ))}
        {sending && (
          <div className="mb-4 flex justify-start">
            <div className="mr-2 mt-1 flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full" style={{ backgroundColor: 'var(--accent)' }}>
              <Sparkles className="h-4 w-4" style={{ color: '#000' }} />
            </div>
            <div className="rounded-2xl px-4 py-3" style={{ backgroundColor: 'var(--bg-elevated)' }}>
              <div className="flex gap-1">
                <div className="h-2 w-2 animate-bounce rounded-full" style={{ backgroundColor: 'var(--text-secondary)', animationDelay: '0ms' }} />
                <div className="h-2 w-2 animate-bounce rounded-full" style={{ backgroundColor: 'var(--text-secondary)', animationDelay: '150ms' }} />
                <div className="h-2 w-2 animate-bounce rounded-full" style={{ backgroundColor: 'var(--text-secondary)', animationDelay: '300ms' }} />
              </div>
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      <div className="border-t px-4 py-3" style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-primary)' }}>
        <div className="flex items-center gap-3">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void handleSend(); } }}
            placeholder="Try: draft a post about… or schedule draft <id> at …"
            className="flex-1 rounded-full border bg-transparent px-4 py-2.5 text-[15px] outline-none transition-colors focus:border-[var(--accent)]"
            style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
            disabled={sending}
          />
          <button
            type="button"
            onClick={() => void handleSend()}
            disabled={!input.trim() || sending}
            className="flex h-10 w-10 items-center justify-center rounded-full transition-opacity disabled:opacity-30"
            style={{ backgroundColor: 'var(--accent)' }}
            aria-label="Send message"
          >
            <Send className="h-5 w-5" style={{ color: '#000' }} />
          </button>
        </div>
      </div>
    </div>
  );
}

/** Convert datetime-local value to ISO UTC for the API. */
function localInputToIso(localValue: string): string {
  const d = new Date(localValue);
  if (Number.isNaN(d.getTime())) return localValue;
  return d.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function DraftsTab({ authEnabled, isSignedIn, getToken }: { authEnabled: boolean; isSignedIn: boolean; getToken: () => Promise<string | null> }) {
  const [drafts, setDrafts] = useState<PulseDraft[]>([]);
  const [schedules, setSchedules] = useState<PulseSchedule[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [filter, setFilter] = useState<string>('pending');
  const [auditByDraft, setAuditByDraft] = useState<Record<string, PulseAuditEntry[]>>({});
  const [auditLoadingId, setAuditLoadingId] = useState<string | null>(null);
  const [auditErrorByDraft, setAuditErrorByDraft] = useState<Record<string, string>>({});
  const [auditOpenId, setAuditOpenId] = useState<string | null>(null);
  const [scheduleAtByDraft, setScheduleAtByDraft] = useState<Record<string, string>>({});
  const [scheduleMsg, setScheduleMsg] = useState<string | null>(null);

  const loadDrafts = useCallback(async () => {
    if (!isSignedIn || !authEnabled) return;
    setLoading(true);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      const [draftResult, schedResult] = await Promise.all([
        listDrafts(token, filter === 'all' ? undefined : filter),
        listSchedules(token),
      ]);
      setDrafts(draftResult.drafts);
      setSchedules(schedResult.schedules.filter((s) => s.status === 'scheduled'));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load drafts');
    } finally {
      setLoading(false);
    }
  }, [getToken, isSignedIn, authEnabled, filter]);

  useEffect(() => { void loadDrafts(); }, [loadDrafts]);

  const actionErrorMessage = (err: unknown, fallback: string) => {
    const status = err instanceof PulseApiError ? err.status : null;
    if (status === 409 || (err instanceof PulseApiError && err.code === 'ILLEGAL_TRANSITION')) {
      return pulseTransitionErrorMessage(err, status);
    }
    // Credit errors only apply to create; still surface message honestly.
    if (status === 402) {
      return pulseCreditErrorMessage(err, status);
    }
    return err instanceof Error ? err.message : fallback;
  };

  const handleApprove = async (id: string) => {
    setActionLoading(id);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      // Approve only then publish — two real server steps (not a fake combined API).
      await approveDraft(token, id);
      await publishDraft(token, id);
      setDrafts((prev) => prev.filter((d) => d.id !== id));
      setAuditByDraft((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      if (auditOpenId === id) setAuditOpenId(null);
    } catch (err) {
      setError(actionErrorMessage(err, 'Approve & publish failed'));
      await loadDrafts();
    } finally {
      setActionLoading(null);
    }
  };

  const handleDismiss = async (id: string) => {
    setActionLoading(id);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      await rejectDraft(token, id, 'Dismissed');
      setDrafts((prev) => prev.filter((d) => d.id !== id));
      setAuditByDraft((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      if (auditOpenId === id) setAuditOpenId(null);
    } catch (err) {
      setError(actionErrorMessage(err, 'Reject failed'));
      await loadDrafts();
    } finally {
      setActionLoading(null);
    }
  };

  const handleApproveOnly = async (id: string) => {
    setActionLoading(id);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      await approveDraft(token, id);
      await loadDrafts();
    } catch (err) {
      setError(actionErrorMessage(err, 'Approve failed'));
      await loadDrafts();
    } finally {
      setActionLoading(null);
    }
  };

  const handleSchedule = async (id: string) => {
    const local = scheduleAtByDraft[id];
    if (!local) {
      setError('Pick a date and time to schedule');
      return;
    }
    setActionLoading(id);
    setScheduleMsg(null);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      const publishAt = localInputToIso(local);
      const result = await scheduleDraft(token, id, publishAt);
      setScheduleMsg(`Scheduled for ${new Date(result.schedule.publishAt).toLocaleString()}`);
      setScheduleAtByDraft((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      await loadDrafts();
    } catch (err) {
      setError(actionErrorMessage(err, 'Schedule failed'));
    } finally {
      setActionLoading(null);
    }
  };

  const handlePublishApproved = async (id: string) => {
    setActionLoading(id);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      await publishDraft(token, id);
      setDrafts((prev) => prev.filter((d) => d.id !== id));
      await loadDrafts();
    } catch (err) {
      setError(actionErrorMessage(err, 'Publish failed'));
      await loadDrafts();
    } finally {
      setActionLoading(null);
    }
  };

  const handleAudit = async (id: string) => {
    if (auditOpenId === id) {
      setAuditOpenId(null);
      return;
    }
    setAuditOpenId(id);
    setAuditLoadingId(id);
    setAuditErrorByDraft((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
    try {
      const token = await getToken();
      if (!token) {
        setAuditErrorByDraft((prev) => ({ ...prev, [id]: 'Sign in again to load audit history.' }));
        return;
      }
      const result = await getDraftAudit(token, id);
      setAuditByDraft((prev) => ({ ...prev, [id]: result.audit }));
    } catch (err) {
      setAuditErrorByDraft((prev) => ({
        ...prev,
        [id]: err instanceof Error ? err.message : 'Failed to load audit',
      }));
    } finally {
      setAuditLoadingId(null);
    }
  };

  if (!authEnabled || !isSignedIn) {
    return (
      <div className="flex flex-col items-center py-16 text-center px-4">
        <Sparkles className="mb-4 h-10 w-10" style={{ color: 'var(--text-secondary)' }} />
        <p className="text-[17px] font-bold mb-1" style={{ color: 'var(--text-primary)' }}>Sign in to view drafts</p>
        <p className="text-[15px] mb-4" style={{ color: 'var(--text-secondary)' }}>
          Drafts you create with Vera appear here for approve, dismiss, or publish.
        </p>
        {authEnabled && <SignInButton mode="modal"><button type="button" className="rounded-full px-6 py-2.5 text-[15px] font-bold" style={{ backgroundColor: 'var(--accent)', color: '#000' }}>Sign in</button></SignInButton>}
      </div>
    );
  }

  return (
    <div className="px-4 py-4">
      <div className="mb-4 flex items-center justify-between">
        <div className="flex gap-2">
          {['pending', 'approved', 'rejected', 'all'].map((f) => (
            <button
              key={f}
              type="button"
              onClick={() => setFilter(f)}
              className="rounded-full border px-3 py-1 text-[13px] font-medium transition-colors"
              style={{
                borderColor: filter === f ? 'var(--accent)' : 'var(--border-secondary)',
                color: filter === f ? 'var(--accent)' : 'var(--text-secondary)',
                backgroundColor: filter === f ? 'color-mix(in srgb, var(--accent) 10%, transparent)' : 'transparent',
              }}
            >
              {f.charAt(0).toUpperCase() + f.slice(1)}
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={() => void loadDrafts()}
          className="rounded-full p-2 transition-colors hover-overlay"
          style={{ color: 'var(--text-secondary)' }}
          aria-label="Refresh"
        >
          <RefreshCw className="h-4 w-4" />
        </button>
      </div>

      {error && <p className="mb-3 text-[13px]" style={{ color: 'var(--color-danger)' }}>{error}</p>}
      {scheduleMsg && <p className="mb-3 text-[13px]" style={{ color: 'var(--accent)' }}>{scheduleMsg}</p>}

      {schedules.length > 0 && (
        <div
          className="mb-4 rounded-xl border p-3"
          style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)' }}
        >
          <p className="mb-2 text-[12px] font-semibold uppercase tracking-wide" style={{ color: 'var(--text-secondary)' }}>
            Upcoming schedules
          </p>
          <ul className="space-y-1.5">
            {schedules.map((s) => (
              <li key={s.id} className="flex flex-wrap items-baseline justify-between gap-2 text-[13px]">
                <span style={{ color: 'var(--text-primary)' }}>
                  Draft <span className="font-mono text-[12px]">{s.draftId.slice(0, 8)}…</span>
                </span>
                <time style={{ color: 'var(--text-secondary)' }} dateTime={s.publishAt}>
                  {new Date(s.publishAt).toLocaleString()}
                </time>
              </li>
            ))}
          </ul>
          <p className="mt-2 text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
            {PULSE_SCHEDULE_PROCESS_ENDPOINT_HINT} {PULSE_SCHEDULE_PROCESSOR_NOTE}
          </p>
        </div>
      )}

      {loading ? (
        <div className="flex justify-center py-8">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-transparent" style={{ borderTopColor: 'var(--accent)' }} />
        </div>
      ) : drafts.length === 0 ? (
        <div className="flex flex-col items-center py-12 text-center">
          <p className="text-[15px] font-medium" style={{ color: 'var(--text-primary)' }}>No {filter !== 'all' ? filter : ''} drafts</p>
          <p className="mt-1 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            Use the Draft helper tab to create a draft, then approve or publish it here.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {drafts.map((draft) => {
            const auditOpen = auditOpenId === draft.id;
            const auditEntries = auditByDraft[draft.id];
            const auditErr = auditErrorByDraft[draft.id];
            const auditLoading = auditLoadingId === draft.id;

            return (
              <div key={draft.id} className="rounded-xl border p-4" style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)' }}>
                <p className="text-[15px] leading-relaxed" style={{ color: 'var(--text-primary)' }}>{draft.body}</p>
                <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
                  <span className="text-[12px]" style={{ color: 'var(--text-tertiary)' }}>
                    {new Date(draft.createdAt).toLocaleDateString()} · {draft.status}
                  </span>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => void handleAudit(draft.id)}
                      disabled={auditLoading}
                      className="rounded-full border px-3 py-1 text-[13px] font-bold transition-colors hover-overlay disabled:opacity-50"
                      style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
                      aria-expanded={auditOpen}
                      aria-controls={`draft-audit-${draft.id}`}
                    >
                      {auditLoading ? 'Loading…' : auditOpen ? 'Hide audit' : 'Audit'}
                    </button>
                    {canPulseDraftAction(draft.status, 'approve') && (
                      <button
                        type="button"
                        onClick={() => void handleApproveOnly(draft.id)}
                        disabled={actionLoading === draft.id}
                        className="rounded-full border px-3 py-1 text-[13px] font-bold transition-colors hover-overlay disabled:opacity-50"
                        style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
                        title="Mark approved only — does not publish"
                      >
                        {pulseDraftActionLabel('approve')}
                      </button>
                    )}
                    {canPulseDraftAction(draft.status, 'approveAndPublish') && (
                      <button
                        type="button"
                        onClick={() => void handleApprove(draft.id)}
                        disabled={actionLoading === draft.id}
                        className="rounded-full px-3 py-1 text-[13px] font-bold transition-opacity hover:opacity-90 disabled:opacity-50"
                        style={{ backgroundColor: 'var(--accent)', color: '#000' }}
                        title="Approve then publish as two server steps"
                      >
                        {actionLoading === draft.id
                          ? 'Publishing…'
                          : pulseDraftActionLabel('approveAndPublish')}
                      </button>
                    )}
                    {canPulseDraftAction(draft.status, 'publish') && (
                      <button
                        type="button"
                        onClick={() => void handlePublishApproved(draft.id)}
                        disabled={actionLoading === draft.id}
                        className="rounded-full px-3 py-1 text-[13px] font-bold transition-opacity hover:opacity-90 disabled:opacity-50"
                        style={{ backgroundColor: 'var(--accent)', color: '#000' }}
                      >
                        {actionLoading === draft.id
                          ? 'Publishing…'
                          : pulseDraftActionLabel('publish')}
                      </button>
                    )}
                    {canPulseDraftAction(draft.status, 'reject') && (
                      <button
                        type="button"
                        onClick={() => void handleDismiss(draft.id)}
                        disabled={actionLoading === draft.id}
                        className="rounded-full border px-3 py-1 text-[13px] font-bold transition-colors hover-overlay disabled:opacity-50"
                        style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
                      >
                        {pulseDraftActionLabel('reject')}
                      </button>
                    )}
                  </div>
                </div>

                {canPulseDraftAction(draft.status, 'schedule') && (
                  <div
                    className="mt-3 flex flex-wrap items-end gap-2 border-t pt-3"
                    style={{ borderColor: 'var(--border-primary)' }}
                  >
                    <label className="flex min-w-[12rem] flex-1 flex-col gap-1">
                      <span className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
                        Schedule publish (approved drafts only)
                      </span>
                      <input
                        type="datetime-local"
                        value={scheduleAtByDraft[draft.id] ?? ''}
                        onChange={(e) =>
                          setScheduleAtByDraft((prev) => ({ ...prev, [draft.id]: e.target.value }))
                        }
                        className="rounded-lg border bg-transparent px-3 py-2 text-[13px] outline-none focus:border-[var(--accent)]"
                        style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() => void handleSchedule(draft.id)}
                      disabled={actionLoading === draft.id || !scheduleAtByDraft[draft.id]}
                      className="rounded-full border px-3 py-2 text-[13px] font-bold transition-colors hover-overlay disabled:opacity-50"
                      style={{ borderColor: 'var(--accent)', color: 'var(--accent)' }}
                    >
                      {actionLoading === draft.id
                        ? 'Scheduling…'
                        : pulseDraftActionLabel('schedule')}
                    </button>
                  </div>
                )}

                {auditOpen && (
                  <div
                    id={`draft-audit-${draft.id}`}
                    className="mt-3 border-t pt-3"
                    style={{ borderColor: 'var(--border-primary)' }}
                  >
                    <p className="mb-2 text-[12px] font-semibold uppercase tracking-wide" style={{ color: 'var(--text-secondary)' }}>
                      Action history
                    </p>
                    {auditLoading && !auditEntries ? (
                      <p className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>Loading audit…</p>
                    ) : auditErr ? (
                      <p className="text-[13px]" style={{ color: 'var(--color-danger)' }}>{auditErr}</p>
                    ) : !auditEntries || auditEntries.length === 0 ? (
                      <p className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>No audit events yet.</p>
                    ) : (
                      <ul className="space-y-2">
                        {auditEntries.map((entry) => (
                          <li
                            key={entry.id}
                            className="rounded-lg border px-3 py-2 text-[13px]"
                            style={{ borderColor: 'var(--border-secondary)', backgroundColor: 'var(--bg-primary)' }}
                          >
                            <div className="flex flex-wrap items-baseline justify-between gap-2">
                              <span className="font-semibold" style={{ color: 'var(--text-primary)' }}>{entry.action}</span>
                              <time className="text-[12px]" style={{ color: 'var(--text-tertiary)' }} dateTime={entry.createdAt}>
                                {new Date(entry.createdAt).toLocaleString()}
                              </time>
                            </div>
                            <p className="mt-0.5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                              Actor: {entry.actorProfileId}
                            </p>
                            {entry.details && Object.keys(entry.details).length > 0 && (
                              <pre
                                className="mt-1 overflow-x-auto whitespace-pre-wrap break-words text-[11px]"
                                style={{ color: 'var(--text-tertiary)' }}
                              >
                                {JSON.stringify(entry.details)}
                              </pre>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Dedicated schedule list (also surfaces on Drafts for approved drafts). */
function ScheduleTab({
  authEnabled,
  isSignedIn,
  getToken,
}: {
  authEnabled: boolean;
  isSignedIn: boolean;
  getToken: () => Promise<string | null>;
}) {
  const [schedules, setSchedules] = useState<PulseSchedule[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!authEnabled || !isSignedIn) return;
    setLoading(true);
    setError(null);
    try {
      const token = await getToken();
      if (!token) return;
      const result = await listSchedules(token);
      setSchedules(result.schedules ?? []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load schedules');
    } finally {
      setLoading(false);
    }
  }, [authEnabled, getToken, isSignedIn]);

  useEffect(() => {
    void load();
  }, [load]);

  if (!authEnabled || !isSignedIn) {
    return (
      <div className="flex flex-col items-center px-4 py-16 text-center">
        <p className="mb-1 text-[17px] font-bold" style={{ color: 'var(--text-primary)' }}>
          Sign in to view schedules
        </p>
        <p className="mb-4 text-[15px]" style={{ color: 'var(--text-secondary)' }}>
          Approved drafts you schedule appear here until publish time.
        </p>
        {authEnabled && (
          <SignInButton mode="modal">
            <button
              type="button"
              className="rounded-full px-6 py-2.5 text-[15px] font-bold"
              style={{ backgroundColor: 'var(--accent)', color: '#000' }}
            >
              Sign in
            </button>
          </SignInButton>
        )}
      </div>
    );
  }

  const upcoming = schedules.filter((s) => s.status === 'scheduled');
  const other = schedules.filter((s) => s.status !== 'scheduled');

  return (
    <div className="px-4 py-4">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h2 className="text-[17px] font-bold">Scheduled posts</h2>
          <p className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            {PULSE_SCHEDULE_PROCESSOR_NOTE}
          </p>
          <p className="mt-1 text-[12px]" style={{ color: 'var(--text-tertiary)' }}>
            {pulseScheduleProcessorStatusLabel()} · Schedule from Drafts after approve.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void load()}
          className="rounded-full p-2 transition-colors hover-overlay"
          style={{ color: 'var(--text-secondary)' }}
          aria-label="Refresh schedules"
        >
          <RefreshCw className="h-4 w-4" />
        </button>
      </div>

      {error && (
        <p className="mb-3 text-[13px]" style={{ color: 'var(--color-danger)' }}>
          {error}
        </p>
      )}

      {loading ? (
        <div className="flex justify-center py-8">
          <div
            className="h-6 w-6 animate-spin rounded-full border-2 border-transparent"
            style={{ borderTopColor: 'var(--accent)' }}
          />
        </div>
      ) : upcoming.length === 0 && other.length === 0 ? (
        <div className="flex flex-col items-center py-12 text-center">
          <p className="text-[15px] font-medium" style={{ color: 'var(--text-primary)' }}>
            No scheduled posts
          </p>
          <p className="mt-1 max-w-sm text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            {PULSE_SCHEDULE_EMPTY_DETAIL}
          </p>
        </div>
      ) : (
        <ul className="space-y-2">
          {[...upcoming, ...other].map((s) => (
            <li
              key={s.id}
              className="rounded-xl border px-4 py-3"
              style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)' }}
            >
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <span className="font-mono text-[13px]" style={{ color: 'var(--text-primary)' }}>
                  Draft {s.draftId.slice(0, 8)}…
                </span>
                <span
                  className="rounded-full px-2 py-0.5 text-[11px] font-semibold uppercase"
                  style={{
                    backgroundColor: 'var(--border-primary)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  {s.status}
                </span>
              </div>
              <time className="mt-1 block text-[13px]" style={{ color: 'var(--text-secondary)' }} dateTime={s.publishAt}>
                {new Date(s.publishAt).toLocaleString()}
              </time>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function GoalsTab({
  authEnabled,
  isSignedIn,
  getToken,
}: {
  authEnabled: boolean;
  isSignedIn: boolean;
  getToken: () => Promise<string | null>;
}) {
  const [goals, setGoals] = useState<PulseGoal[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const loadGoals = useCallback(async () => {
    if (!authEnabled || !isSignedIn) {
      setGoals([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const token = await getToken();
      if (!token) {
        setError('Could not verify session.');
        setGoals([]);
        return;
      }
      const res = await listGoals(token);
      setGoals(res.goals ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load goals');
      setGoals([]);
    } finally {
      setLoading(false);
    }
  }, [authEnabled, isSignedIn, getToken]);

  useEffect(() => {
    void loadGoals();
  }, [loadGoals]);

  const handleCreate = async () => {
    const text = input.trim();
    if (!text || creating) return;
    if (!authEnabled || !isSignedIn) {
      setError('Sign in to create a goal plan.');
      return;
    }
    setCreating(true);
    setError(null);
    try {
      const token = await getToken();
      if (!token) {
        setError('Could not verify session.');
        return;
      }
      const res = await createGoal(token, text);
      setInput('');
      setExpandedId(res.goal.id);
      await loadGoals();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create goal');
    } finally {
      setCreating(false);
    }
  };

  if (!authEnabled || !isSignedIn) {
    return (
      <div className="px-4 py-12 text-center">
        <p className="mb-4 text-[15px]" style={{ color: 'var(--text-secondary)' }}>
          Sign in to create a goal plan template (deterministic steps — not Temporal autopilot).
        </p>
        {authEnabled && (
          <SignInButton mode="modal">
            <button
              type="button"
              className="rounded-full px-5 py-2 text-[14px] font-bold"
              style={{ backgroundColor: 'var(--accent)', color: '#000' }}
            >
              Sign in
            </button>
          </SignInButton>
        )}
      </div>
    );
  }

  return (
    <div className="px-4 py-6">
      <div className="mx-auto max-w-lg space-y-5">
        <div>
          <h2 className="mb-1 text-[17px] font-bold" style={{ color: 'var(--text-primary)' }}>
            Goal plans
          </h2>
          <p className="text-[13px] leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
            Deterministic plan template only — not Temporal execution. Steps map to existing tools
            (create draft → human approve → optional schedule). Run them yourself via Drafts.
          </p>
        </div>

        <div className="flex gap-2">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                void handleCreate();
              }
            }}
            placeholder='e.g. post about shipping tools next week'
            className="flex-1 rounded-full border bg-transparent px-4 py-2.5 text-[14px] outline-none focus:border-[var(--accent)]"
            style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-primary)' }}
            disabled={creating}
          />
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={creating || !input.trim()}
            className="rounded-full px-4 py-2 text-[13px] font-bold disabled:opacity-50"
            style={{ backgroundColor: 'var(--accent)', color: '#000' }}
          >
            {creating ? '…' : 'Plan'}
          </button>
        </div>

        {error && (
          <p className="text-[13px]" style={{ color: 'var(--color-danger)' }}>
            {error}
          </p>
        )}

        <div className="flex items-center justify-between">
          <p className="text-[12px] font-semibold uppercase tracking-wide" style={{ color: 'var(--text-secondary)' }}>
            Your plans
          </p>
          <button
            type="button"
            onClick={() => void loadGoals()}
            className="inline-flex items-center gap-1 text-[12px]"
            style={{ color: 'var(--text-secondary)' }}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </button>
        </div>

        {loading && goals.length === 0 ? (
          <p className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            Loading…
          </p>
        ) : goals.length === 0 ? (
          <p className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            No goals yet. Try: “post about X next week”.
          </p>
        ) : (
          <ul className="space-y-3">
            {goals.map((g) => {
              const open = expandedId === g.id;
              return (
                <li
                  key={g.id}
                  className="rounded-xl border p-4"
                  style={{ borderColor: 'var(--border-primary)', backgroundColor: 'var(--bg-elevated)' }}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    onClick={() => setExpandedId(open ? null : g.id)}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <p className="text-[15px] font-medium" style={{ color: 'var(--text-primary)' }}>
                        {g.goal}
                      </p>
                      <span
                        className="flex-shrink-0 rounded-full border px-2 py-0.5 text-[11px]"
                        style={{ borderColor: 'var(--border-secondary)', color: 'var(--text-tertiary)' }}
                      >
                        {g.status}
                      </span>
                    </div>
                    <p className="mt-1 text-[12px]" style={{ color: 'var(--text-tertiary)' }}>
                      {(g.steps?.length ?? 0)} step{(g.steps?.length ?? 0) === 1 ? '' : 's'} · plan template
                    </p>
                  </button>
                  {open && (
                    <ol className="mt-3 space-y-2 border-t pt-3" style={{ borderColor: 'var(--border-primary)' }}>
                      {(g.steps ?? []).map((step, idx) => (
                        <li
                          key={`${g.id}-${idx}`}
                          className="rounded-lg border px-3 py-2 text-[13px]"
                          style={{ borderColor: 'var(--border-secondary)', backgroundColor: 'var(--bg-primary)' }}
                        >
                          <div className="flex flex-wrap items-baseline justify-between gap-2">
                            <span className="font-semibold" style={{ color: 'var(--text-primary)' }}>
                              {idx + 1}. {step.tool}
                            </span>
                            <span className="text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
                              {step.status}
                            </span>
                          </div>
                          <p className="mt-0.5" style={{ color: 'var(--text-secondary)' }}>
                            {step.description}
                          </p>
                          {step.args && Object.keys(step.args).length > 0 && (
                            <pre
                              className="mt-1 overflow-x-auto whitespace-pre-wrap break-words text-[11px]"
                              style={{ color: 'var(--text-tertiary)' }}
                            >
                              {JSON.stringify(step.args)}
                            </pre>
                          )}
                        </li>
                      ))}
                    </ol>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}


