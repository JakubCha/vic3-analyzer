import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AdvisorPane, loadTracked } from './AdvisorPane'
import type { AdviceResult } from './types'
import type { WasmApi } from './wasm'

const advice: AdviceResult = {
  advice: [
    {
      id: 'goods:iron:build',
      domain: 'economy',
      priority: 1,
      title: 'Build an iron mine in Silesia',
      detail: 'Iron is short.',
      check: { type: 'building_levels', building_type: 'building_iron_mine', state_id: 3, at_least: 4 },
    },
    {
      id: 'law:law_serfdom',
      domain: 'laws',
      priority: 1,
      title: 'Abolish serfdom',
      detail: 'Serfs are tied to the land.',
      check: { type: 'law_inactive', law: 'law_serfdom' },
    },
  ],
  limitations: ['Laws advice is a vanilla heuristic.'],
}

function fakeApi(done: boolean[]) {
  return {
    loaded_advice: vi.fn(async () => JSON.stringify(advice)),
    loaded_checks: vi.fn(async () => JSON.stringify(done)),
  } as unknown as WasmApi
}

describe('AdvisorPane', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('filters by domain and tracks a suggestion until a save confirms it', async () => {
    const user = userEvent.setup()
    const first = fakeApi([])
    const { rerender } = render(
      <AdvisorPane api={first} analysisKey={1} tag="PRU" date="1840.1.1" />,
    )
    expect(await screen.findByText('Build an iron mine in Silesia')).toBeInTheDocument()
    expect(screen.queryByText('Abolish serfdom')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Do this' }))
    expect(screen.getByText(/Waiting for a save/)).toBeInTheDocument()
    expect(loadTracked()).toHaveLength(1)

    await user.click(screen.getByRole('tab', { name: /Laws/ }))
    expect(screen.getByText('Abolish serfdom')).toBeInTheDocument()

    // Next autosave: the check holds.
    const next = fakeApi([true])
    rerender(<AdvisorPane api={next} analysisKey={2} tag="PRU" date="1840.2.1" />)
    expect(await screen.findByText(/Done \(1840\.2\.1\)/)).toBeInTheDocument()
    expect(next.loaded_checks).toHaveBeenCalledWith(
      JSON.stringify([advice.advice[0].check]),
    )
    await waitFor(() => expect(loadTracked()[0].doneAt).toBe('1840.2.1'))
  })

  it('keeps other countries’ tracked items out of view', async () => {
    localStorage.setItem(
      'vic3-advisor-tracked',
      JSON.stringify([{ advice: advice.advice[1], tag: 'GBR' }]),
    )
    render(<AdvisorPane api={fakeApi([])} analysisKey={1} tag="PRU" />)
    await screen.findByText('Build an iron mine in Silesia')
    expect(screen.queryByText('Your to-do list')).not.toBeInTheDocument()
  })
})
