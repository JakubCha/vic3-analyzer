import { useEffect, useState } from 'react'
import type { RenderedChart } from './types'
import type { WasmApi } from './wasm'

/** Same order as `PALETTE` in vic3_api::history. */
const PALETTE = ['#2a4a6a', '#c06020', '#2f6b4f', '#963c78']

function format(unit: RenderedChart['unit'], v: number): string {
  const a = Math.abs(v)
  if (unit === 'share') return `${Math.round(v * 100)}%`
  if (a >= 1e9) return `${(v / 1e9).toFixed(1)}B`
  if (a >= 1e6) return `${(v / 1e6).toFixed(1)}M`
  if (a >= 1e3) return `${(v / 1e3).toFixed(1)}K`
  return unit === 'plain' && a < 100 ? v.toFixed(1) : v.toFixed(0)
}

function change(unit: RenderedChart['unit'], first?: number | null, last?: number | null): string {
  if (first == null || last == null) return ''
  if (unit === 'share') return `${last >= first ? '+' : ''}${Math.round((last - first) * 100)} pp`
  if (first === 0) return ''
  const pct = ((last - first) / Math.abs(first)) * 100
  return `${pct >= 0 ? '+' : ''}${pct.toFixed(0)}%`
}

interface Props {
  api: WasmApi
  /** Changes whenever a new save is analysed. */
  analysisKey: unknown
}

/** Charts over time: budget from the mod's weekly log, the rest from the save's own histories. */
export function HistoryPane({ api, analysisKey }: Props) {
  const [charts, setCharts] = useState<RenderedChart[]>()
  const [error, setError] = useState<string>()

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const next = JSON.parse(await api.loaded_history()) as RenderedChart[]
        if (!cancelled) {
          setCharts(next)
          setError(undefined)
        }
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api, analysisKey])

  if (error) return <p role="alert">{error}</p>
  if (!charts) return <p>Drawing charts…</p>
  const hasBudget = charts.some((c) => c.key === 'budget')
  return (
    <div className="history">
      {!hasBudget && (
        <p className="alert-severity">
          Budget history appears once the Oikonomia Advisor mod has logged two weeks of play (it reads the game log on
          this computer).
        </p>
      )}
      <div className="history-grid">
        {charts.map((chart) => (
          <figure key={chart.key} className="history-card" aria-label={chart.title}>
            <figcaption>
              <strong>{chart.title}</strong>
              <ul className="history-legend">
                {chart.lines.map((line, i) => (
                  <li key={line.label}>
                    <span className="history-swatch" style={{ background: PALETTE[i % PALETTE.length] }} />
                    {line.label} {line.last != null && <b>{format(chart.unit, line.last)}</b>}{' '}
                    <span className="alert-severity">{change(chart.unit, line.first, line.last)}</span>
                  </li>
                ))}
              </ul>
            </figcaption>
            {/* SVG drawn by our own Rust (vic3_api::history), not user input. */}
            <div className="history-svg" dangerouslySetInnerHTML={{ __html: chart.svg }} />
          </figure>
        ))}
      </div>
    </div>
  )
}
