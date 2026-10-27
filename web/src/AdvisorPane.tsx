import { useEffect, useState } from 'react'
import type { Advice, AdviceDomain, AdviceResult } from './types'
import type { WasmApi } from './wasm'

const DOMAINS: readonly { id: AdviceDomain; label: string }[] = [
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

const STORAGE_KEY = 'vic3-advisor-tracked'

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

/** Advisor: per-domain suggestions plus a tracked list confirmed by later saves. */
export function AdvisorPane({ api, analysisKey, tag, date }: Props) {
  const [domain, setDomain] = useState<AdviceDomain>('economy')
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
  const list = (result?.advice ?? []).filter((item) => item.domain === domain)

  const track = (advice: Advice) => update([...tracked, { advice, tag, trackedAt: date }])
  const markDone = (target: TrackedAdvice) =>
    update(tracked.map((item) => (item === target ? { ...item, doneAt: date ?? 'done' } : item)))
  const remove = (target: TrackedAdvice) => update(tracked.filter((item) => item !== target))

  return (
    <div className="advisor">
      <div className="advisor-domains" role="tablist" aria-label="Advisor domain">
        {DOMAINS.map((item) => {
          const count = (result?.advice ?? []).filter((a) => a.domain === item.id).length
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

      {error && <p role="alert">{error}</p>}
      {!result && !error && <p>Preparing advice…</p>}
      {result && list.length === 0 && <p>Nothing to suggest here for this save.</p>}

      <ol className="advisor-list" aria-label="Suggestions">
        {list.map((item) => (
          <li key={item.id} className={`advisor-item priority-${item.priority}`}>
            <span className="alert-severity">{item.priority === 1 ? 'Urgent' : 'Worthwhile'}</span>{' '}
            <strong>{item.title}</strong>
            <p>{item.detail}</p>
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
