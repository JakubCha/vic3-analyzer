import { useEffect, useState } from 'react'
import { AdviceExtras, adviceInTab, BudgetBanner, loadGoals, money } from './AdvisorPane'
import type { AdviceResult, LiveStatus } from './types'
import type { WasmApi } from './wasm'

/** Event the desktop app emits after a save is loaded (`ANALYSIS_LOADED_EVENT`). */
export const ANALYSIS_LOADED = 'analysis-loaded'
/** How often the overlay rereads the mod's live line from the game log. */
export const LIVE_POLL_MS = 5000

type Listen = (event: string, handler: () => void) => Promise<() => void>

interface Props {
  api: WasmApi
  /** Tauri's `event.listen`; absent outside the desktop app. */
  listen?: Listen
  /** Reads the mod's newest in-game line (`live_status`); absent outside the desktop app. */
  live?: () => Promise<LiveStatus | null>
}

/** In-game numbers from the mod, newer than any save. */
const LIVE_TOP = 3

/** Labels of the first few items, comma separated. */
function top<T>(items: T[], label: (item: T) => string) {
  return items.slice(0, LIVE_TOP).map(label).join(', ')
}

function LiveBar({ live: s }: { live: LiveStatus }) {
  const states = s.states ?? []
  const short = states.filter((st) => st.infrastructure_balance < 0).sort((a, b) => a.infrastructure_balance - b.infrastructure_balance)
  const jobless = states.filter((st) => st.unemployed > 0).sort((a, b) => b.unemployed - a.unemployed)
  const goods = [...(s.goods ?? [])].sort((a, b) => b.price_vs_base - a.price_vs_base)
  const dear = goods.filter((g) => g.price_vs_base > 0)
  const cheap = goods.filter((g) => g.price_vs_base < 0).reverse()
  const pct = (x: number) => `${x > 0 ? '+' : '−'}${Math.round(Math.abs(x) * 100)}%`
  return (
    <div className="advisor-live" aria-label="Live">
      <p>
        <strong>In game {s.date}</strong> · balance{' '}
        {s.weekly_balance_total != null ? (
          <>
            {money(s.weekly_balance_total)}/week ({money(s.weekly_balance)} without construction)
          </>
        ) : (
          <>{money(s.weekly_balance)}/week</>
        )}{' '}
        · treasury {money(s.treasury)}
        {s.weekly_balance < 0 && s.debt_share === 0 && <> · lasts ~{s.weeks_to_debt} weeks</>}
        {s.debt_share > 0 && <> · debt {Math.round(s.debt_share * 100)}% of credit</>} · military{' '}
        {Math.round(s.military_share * 100)}% of spending
      </p>
      {short.length > 0 && <p>Short of infrastructure: {top(short, (st) => `${st.name} (${st.infrastructure_balance})`)}</p>}
      {jobless.length > 0 && <p>Unemployed: {top(jobless, (st) => `${st.name} (${money(st.unemployed)})`)}</p>}
      {dear.length > 0 && <p>Dear: {top(dear, (g) => `${g.name} ${pct(g.price_vs_base)}`)}</p>}
      {cheap.length > 0 && <p>Cheap: {top(cheap, (g) => `${g.name} ${pct(g.price_vs_base)}`)}</p>}
    </div>
  )
}

/** Compact always-on-top window: live line, budget bar and the Now list, refreshed per loaded save. */
export function OverlayView({ api, listen, live }: Props) {
  const [result, setResult] = useState<AdviceResult>()
  const [error, setError] = useState<string>()
  const [liveStatus, setLiveStatus] = useState<LiveStatus>()

  useEffect(() => {
    if (!live) return
    let cancelled = false
    const poll = async () => {
      // Keep the last line when the log is busy or unreadable.
      const next = await live().catch(() => null)
      if (!cancelled && next) setLiveStatus(next)
    }
    void poll()
    const timer = setInterval(() => void poll(), LIVE_POLL_MS)
    return () => {
      cancelled = true
      clearInterval(timer)
    }
  }, [live])

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      try {
        const next = JSON.parse(await api.loaded_advice()) as AdviceResult
        if (!cancelled) {
          setResult(next)
          setError(undefined)
        }
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason))
      }
    }
    void load()
    const unlisten = listen?.(ANALYSIS_LOADED, () => void load())
    return () => {
      cancelled = true
      void unlisten?.then((stop) => stop())
    }
  }, [api, listen])

  // Goals are read on every render so a change in the main window applies on the next save.
  const items = adviceInTab(result?.advice ?? [], 'now', loadGoals())
  return (
    <div className="advisor overlay">
      {liveStatus && <LiveBar live={liveStatus} />}
      {error && <p role="alert">Load a save in the main window first. ({error})</p>}
      {result?.budget && <BudgetBanner budget={result.budget} />}
      <ol className="advisor-list" aria-label="Now">
        {items.map((item) => (
          <li key={item.id} className={`advisor-item priority-${item.priority}`}>
            <strong>{item.title}</strong>
            <p>{item.detail}</p>
            <AdviceExtras item={item} />
            {item.why && (
              <details className="advisor-why">
                <summary>How it works</summary>
                <p>{item.why}</p>
              </details>
            )}
          </li>
        ))}
      </ol>
    </div>
  )
}
