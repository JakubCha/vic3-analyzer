import { cleanup, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ANALYSIS_LOADED, OverlayView } from './OverlayView'
import type { AdviceResult } from './types'
import type { WasmApi } from './wasm'

const result = (title: string): AdviceResult => ({
  advice: [{ id: 'budget:balance', domain: 'economy', priority: 2, title, detail: '' }],
  limitations: [],
})

describe('OverlayView', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('shows the Now list and reloads when a save is loaded', async () => {
    const loaded_advice = vi
      .fn()
      .mockResolvedValueOnce(JSON.stringify(result('Deficit 7.2K')))
      .mockResolvedValueOnce(JSON.stringify(result('Deficit 3.1K')))
    let fire: () => void = () => {}
    const listen = vi.fn(async (event: string, handler: () => void) => {
      expect(event).toBe(ANALYSIS_LOADED)
      fire = handler
      return () => {}
    })
    render(<OverlayView api={{ loaded_advice } as unknown as WasmApi} listen={listen} />)
    expect(await screen.findByText('Deficit 7.2K')).toBeInTheDocument()
    await act(async () => fire())
    expect(await screen.findByText('Deficit 3.1K')).toBeInTheDocument()
  })

  it('shows the live in-game line from the mod', async () => {
    const api = { loaded_advice: vi.fn(async () => JSON.stringify(result('Deficit'))) } as unknown as WasmApi
    const live = vi.fn(async () => ({
      country: 'PRU',
      date: '1 marca 1836',
      logged_at: '21:02:59',
      weekly_balance: -7231,
      weekly_income: 57845,
      treasury: 486803,
      debt_share: 0,
      military_share: 0.618,
      weeks_to_debt: 67,
      gdp: 9720602,
      states: [
        { name: 'Brandenburg', unemployed: 12400, infrastructure_balance: -3 },
        { name: 'Silesia', unemployed: 0, infrastructure_balance: -7 },
        { name: 'Posen', unemployed: 800, infrastructure_balance: 4 },
      ],
      goods: [
        { name: 'Fabric', price_vs_base: -0.25 },
        { name: 'Wood', price_vs_base: 0.71 },
      ],
    }))
    render(<OverlayView api={api} live={live} />)
    const bar = await screen.findByLabelText('Live')
    expect(bar).toHaveTextContent(/In game 1 marca 1836.*−7\.2K\/week.*lasts ~67 weeks.*military 62% of spending/)
    expect(bar).toHaveTextContent('Short of infrastructure: Silesia (-7), Brandenburg (-3)')
    expect(bar).toHaveTextContent('Unemployed: Brandenburg (12.4K), Posen (800)')
    expect(bar).toHaveTextContent('Dear: Wood +71%')
    expect(bar).toHaveTextContent('Cheap: Fabric −25%')
  })

  it('asks for a save when none is loaded', async () => {
    const api = { loaded_advice: vi.fn().mockRejectedValue(new Error('no loaded analysis')) } as unknown as WasmApi
    render(<OverlayView api={api} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Load a save in the main window first')
  })
})
