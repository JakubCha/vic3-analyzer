import { useEffect, useState } from 'react'
import { adviceInTab, BudgetBanner, loadGoals } from './AdvisorPane'
import type { AdviceResult } from './types'
import type { WasmApi } from './wasm'

/** Event the desktop app emits after a save is loaded (`ANALYSIS_LOADED_EVENT`). */
export const ANALYSIS_LOADED = 'analysis-loaded'

type Listen = (event: string, handler: () => void) => Promise<() => void>

interface Props {
  api: WasmApi
  /** Tauri's `event.listen`; absent outside the desktop app. */
  listen?: Listen
}

/** Compact always-on-top window: budget bar and the Now list, refreshed per loaded save. */
export function OverlayView({ api, listen }: Props) {
  const [result, setResult] = useState<AdviceResult>()
  const [error, setError] = useState<string>()

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
      {error && <p role="alert">Load a save in the main window first. ({error})</p>}
      {result?.budget && <BudgetBanner budget={result.budget} />}
      <ol className="advisor-list" aria-label="Now">
        {items.map((item) => (
          <li key={item.id} className={`advisor-item priority-${item.priority}`}>
            <strong>{item.title}</strong>
            <p>{item.detail}</p>
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
