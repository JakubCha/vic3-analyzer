import { useEffect, useState } from 'react'
import type { Advice, AdviceDomain, AdviceGoal, AdviceResult, BudgetReport } from './types'
import { isTauri } from './env'
import { invokeTauri } from './tauriApi'
import type { WasmApi } from './wasm'

/** `now`: the few most important items across every domain. */
export type View = AdviceDomain | 'now'

/** How many items the Now tab shows: a short session's worth. */
const NOW_LIMIT = 5

const DOMAINS: readonly { id: View; label: string }[] = [
  { id: 'now', label: 'Now' },
  { id: 'economy', label: 'Economy' },
  { id: 'laws', label: 'Laws' },
  { id: 'war', label: 'War' },
]

/** A suggestion the player chose to act on, kept across saves. */
export interface TrackedAdvice {
  advice: Advice
  /** Country tag the advice was given for (tracking is per campaign country). */
  tag?: string
  trackedAt?: string
  /** In-game date of the first save where the check held (or the manual tick). */
  doneAt?: string
}

export type GoalDomain = 'economy' | 'laws'

/** Choosable goals per tab, with the hint shown next to the picker. */
const GOALS: Record<GoalDomain, { options: readonly { id: AdviceGoal; label: string }[]; hint: string }> = {
  economy: {
    options: [
      { id: 'growth', label: 'Growth' },
      { id: 'living_standards', label: 'Standard of living' },
      { id: 'industrialization', label: 'Industrialization' },
      { id: 'self_sufficiency', label: 'Self-sufficiency' },
    ],
    hint: 'What the economy should optimise. Loss-makers and gluts show for every goal.',
  },
  laws: {
    options: [
      { id: 'economy', label: 'Economy' },
      { id: 'authority', label: 'Authority' },
      { id: 'democracy', label: 'Democracy' },
    ],
    hint: 'Next step per law group, ranked by interest-group support in your country.',
  },
}

const STORAGE_KEY = 'vic3-advisor-tracked'
const GOAL_KEYS: Record<GoalDomain, string> = {
  economy: 'vic3-advisor-economy-goal',
  laws: 'vic3-advisor-law-goal',
}

function loadGoal(domain: GoalDomain): AdviceGoal {
  const options = GOALS[domain].options
  try {
    const stored = localStorage.getItem(GOAL_KEYS[domain])
    return options.find((goal) => goal.id === stored)?.id ?? options[0].id
  } catch {
    return options[0].id
  }
}

export function loadTracked(): TrackedAdvice[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]')
    return Array.isArray(parsed) ? (parsed as TrackedAdvice[]) : []
  } catch {
    return []
  }
}

function storeTracked(items: TrackedAdvice[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items))
  } catch {
    // Storage blocked: tracking lasts until reload.
  }
}

interface Props {
  api: WasmApi
  /** Changes whenever a new save is analysed (the prices result object). */
  analysisKey: unknown
  tag?: string
  date?: string
}

/** The chosen goal per tab, as stored by the Advisor (the overlay reads the same). */
export function loadGoals(): Record<GoalDomain, AdviceGoal> {
  return { economy: loadGoal('economy'), laws: loadGoal('laws') }
}

/**
 * Advice shown in a tab. Goal-tagged items show only under their tab's chosen
 * goal. Advice arrives sorted by priority (budget, playbook, market within
 * each); Now lifts the budget verdict to the top even when only "worthwhile".
 */
export function adviceInTab(advice: Advice[], tab: View, goals: Record<GoalDomain, AdviceGoal>): Advice[] {
  const inView = (item: Advice) => !item.goal || (item.domain !== 'war' && item.goal === goals[item.domain])
  const items = advice.filter((item) => (tab === 'now' || item.domain === tab) && inView(item))
  if (tab !== 'now') return items
  const budgetFirst = (item: Advice) => (item.id === 'budget:balance' ? 0 : 1)
  return [...items].sort((a, b) => budgetFirst(a) - budgetFirst(b)).slice(0, NOW_LIMIT)
}

/** Advisor: per-domain suggestions plus a tracked list confirmed by later saves. */
export function AdvisorPane({ api, analysisKey, tag, date }: Props) {
  const [domain, setDomain] = useState<View>('now')
  const [goals, setGoals] = useState<Record<GoalDomain, AdviceGoal>>(loadGoals)
  const [result, setResult] = useState<AdviceResult>()
  const [tracked, setTracked] = useState<TrackedAdvice[]>(loadTracked)
  const [error, setError] = useState<string>()

  const update = (next: TrackedAdvice[]) => {
    setTracked(next)
    storeTracked(next)
  }

  useEffect(() => {
    let cancelled = false
    setError(undefined)
    void (async () => {
      try {
        const advice = JSON.parse(await api.loaded_advice()) as AdviceResult
        if (cancelled) return
        setResult(advice)
        // Re-check open tracked items against this save.
        const all = loadTracked()
        const open = all.filter((item) => item.tag === tag && !item.doneAt && item.advice.check)
        if (open.length === 0) return
        const done = JSON.parse(
          await api.loaded_checks(JSON.stringify(open.map((item) => item.advice.check))),
        ) as boolean[]
        if (cancelled) return
        const finished = new Set(open.filter((_, i) => done[i]))
        if (finished.size === 0) return
        update(all.map((item) => (finished.has(item) ? { ...item, doneAt: date ?? 'done' } : item)))
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason))
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `date`/`tag` change with `analysisKey`.
  }, [api, analysisKey])

  const mine = tracked.filter((item) => item.tag === tag)
  const isTracked = (id: string) => mine.some((item) => item.advice.id === id)
  const inTab = (tab: View) => adviceInTab(result?.advice ?? [], tab, goals)
  const list = inTab(domain)
  const goalDomain: GoalDomain | undefined = domain === 'war' || domain === 'now' ? undefined : domain
  const pickGoal = (target: GoalDomain, next: AdviceGoal) => {
    setGoals((current) => ({ ...current, [target]: next }))
    try {
      localStorage.setItem(GOAL_KEYS[target], next)
    } catch {
      // Storage blocked: the choice lasts until reload.
    }
  }

  const track = (advice: Advice) => update([...tracked, { advice, tag, trackedAt: date }])
  const markDone = (target: TrackedAdvice) =>
    update(tracked.map((item) => (item === target ? { ...item, doneAt: date ?? 'done' } : item)))
  const remove = (target: TrackedAdvice) => update(tracked.filter((item) => item !== target))

  return (
    <div className="advisor">
      {result?.budget && <BudgetBanner budget={result.budget} />}

      <div className="advisor-domains" role="tablist" aria-label="Advisor domain">
        {DOMAINS.map((item) => {
          const count = inTab(item.id).length
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={domain === item.id}
              className={domain === item.id ? 'active' : undefined}
              onClick={() => setDomain(item.id)}
            >
              {item.label} ({count})
            </button>
          )
        })}
      </div>

      {goalDomain && (
        <label className="advisor-goal">
          Goal{' '}
          <select
            value={goals[goalDomain]}
            onChange={(event) => pickGoal(goalDomain, event.target.value as AdviceGoal)}
          >
            {GOALS[goalDomain].options.map((item) => (
              <option key={item.id} value={item.id}>
                {item.label}
              </option>
            ))}
          </select>{' '}
          <span className="alert-severity">{GOALS[goalDomain].hint}</span>
        </label>
      )}
      {domain === 'now' && (
        <p className="alert-severity">
          The {NOW_LIMIT} most important decisions right now, budget first. Uses the goals chosen in the Economy and
          Laws tabs.{' '}
          {isTauri() && (
            <button type="button" onClick={() => void invokeTauri<boolean>('toggle_overlay')}>
              Overlay on top of the game
            </button>
          )}
        </p>
      )}

      {error && <p role="alert">{error}</p>}
      {!result && !error && <p>Preparing advice…</p>}
      {result && list.length === 0 && <p>Nothing to suggest here for this save.</p>}

      <ol className="advisor-list" aria-label="Suggestions">
        {list.map((item) => (
          <li key={item.id} className={`advisor-item priority-${item.priority}`}>
            <span className="alert-severity">{item.priority === 1 ? 'Urgent' : 'Worthwhile'}</span>{' '}
            <strong>{item.title}</strong>
            <p>{item.detail}</p>
            {item.why && (
              <details className="advisor-why" open>
                <summary>How it works</summary>
                <p>{item.why}</p>
              </details>
            )}
            <button type="button" disabled={isTracked(item.id)} onClick={() => track(item)}>
              {isTracked(item.id) ? 'Tracked' : 'Do this'}
            </button>
          </li>
        ))}
      </ol>

      {mine.length > 0 && (
        <section aria-label="Tracked suggestions">
          <h3>Your to-do list</h3>
          <ul className="advisor-tracked">
            {mine.map((item) => (
              <li key={item.advice.id}>
                <strong>{item.advice.title}</strong>{' '}
                {item.doneAt ? (
                  <span className="advisor-done">✓ Done ({item.doneAt})</span>
                ) : item.advice.check ? (
                  <span>Waiting for a save that shows it done</span>
                ) : (
                  <button type="button" onClick={() => markDone(item)}>
                    Mark done
                  </button>
                )}{' '}
                <button type="button" onClick={() => remove(item)} aria-label={`Remove ${item.advice.title}`}>
                  Remove
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {result && result.limitations.length > 0 && (
        <details>
          <summary>Limitations</summary>
          <ul>
            {result.limitations.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  )
}

/** Short money figure: 7.2K, 17.1M (same as `vic3_api::budget::money`). */
export function money(x: number): string {
  const a = Math.abs(x)
  const sign = x < 0 ? '−' : ''
  if (a >= 1e6) return `${sign}${(a / 1e6).toFixed(1)}M`
  if (a >= 1e3) return `${sign}${(a / 1e3).toFixed(1)}K`
  return `${sign}${a.toFixed(0)}`
}

const RISK_LABEL: Record<BudgetReport['risk'], string> = {
  safe: 'Budget OK',
  deficit: 'Deficit',
  danger: 'Danger: heading for default',
  critical: 'Critical: default is imminent',
}

/** Budget guard: balance, runway and where the money goes. */
export function BudgetBanner({ budget: b }: { budget: BudgetReport }) {
  const weeks = (w: number) => (w >= 104 ? `${(w / 52).toFixed(1)} years` : `${w.toFixed(0)} weeks`)
  const runway =
    b.debt > 0
      ? `debt ${money(b.debt)}${b.credit_limit != null ? ` of ${money(b.credit_limit)} credit` : ''}`
      : b.weeks_to_debt != null
        ? `treasury lasts ~${weeks(b.weeks_to_debt)}`
        : null
  return (
    <section className={`advisor-budget risk-${b.risk}`} aria-label="Budget">
      <p>
        <strong>{b.risk === 'deficit' && b.deficit_kind === 'investment' ? 'Investment deficit' : RISK_LABEL[b.risk]}</strong>{' '}
        · balance {b.weekly_balance != null ? money(b.weekly_balance) : '?'}/week ·
        treasury {money(b.treasury)}
        {runway && <> · {runway}</>}
        {b.weeks_to_unhealthy != null && b.risk !== 'critical' ? (
          <> · debt at half the credit limit in ~{weeks(b.weeks_to_unhealthy)}</>
        ) : (
          b.weeks_to_default != null && <> · credit limit in ~{weeks(b.weeks_to_default)}</>
        )}
      </p>
      {b.expenses.length > 0 && (
        <ul className="budget-bars" aria-label="Weekly expenses">
          {b.expenses.slice(0, 5).map((g) => (
            <li key={g.group} title={g.top.map(([name, amount]) => `${name} ${money(amount)}`).join(', ')}>
              <span className="budget-bar" style={{ width: `${Math.round(g.share * 100)}%` }} />
              {g.label} {money(g.amount)} ({Math.round(g.share * 100)}%)
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
