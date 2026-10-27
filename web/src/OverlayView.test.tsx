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

  it('asks for a save when none is loaded', async () => {
    const api = { loaded_advice: vi.fn().mockRejectedValue(new Error('no loaded analysis')) } as unknown as WasmApi
    render(<OverlayView api={api} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Load a save in the main window first')
  })
})
