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

  it('shows laws for the chosen goal only and remembers the goal', async () => {
    const user = userEvent.setup()
    const goals: AdviceResult = {
      advice: [
        { id: 'law:authority:x', domain: 'laws', goal: 'authority', priority: 1, title: 'Enact Secret Police', detail: '' },
        { id: 'law:democracy:x', domain: 'laws', goal: 'democracy', priority: 1, title: 'Enact Oligarchy', detail: '' },
      ],
      limitations: [],
    }
    const api = {
      loaded_advice: vi.fn(async () => JSON.stringify(goals)),
      loaded_checks: vi.fn(async () => '[]'),
    } as unknown as WasmApi
    render(<AdvisorPane api={api} analysisKey={1} tag="PRU" />)
    await user.click(await screen.findByRole('tab', { name: /Laws \(0\)/ }))
    await user.selectOptions(screen.getByRole('combobox'), 'democracy')
    expect(screen.getByText('Enact Oligarchy')).toBeInTheDocument()
    expect(screen.queryByText('Enact Secret Police')).not.toBeInTheDocument()
    expect(localStorage.getItem('vic3-advisor-law-goal')).toBe('democracy')
  })

  it('filters economy advice by economy goal and keeps goal-less warnings', async () => {
    const user = userEvent.setup()
    const goals: AdviceResult = {
      advice: [
        { id: 'growth:x', domain: 'economy', goal: 'growth', priority: 1, title: 'Expand Coal Mines', detail: '' },
        { id: 'living_standards:x', domain: 'economy', goal: 'living_standards', priority: 1, title: 'Build more Wheat Farms', detail: '' },
        { id: 'loss:x', domain: 'economy', priority: 2, title: 'Fix the loss-making Shipyards', detail: '' },
      ],
      limitations: [],
    }
    const api = {
      loaded_advice: vi.fn(async () => JSON.stringify(goals)),
      loaded_checks: vi.fn(async () => '[]'),
    } as unknown as WasmApi
    render(<AdvisorPane api={api} analysisKey={1} tag="PRU" />)
    expect(await screen.findByText('Expand Coal Mines')).toBeInTheDocument()
    expect(screen.queryByText('Build more Wheat Farms')).not.toBeInTheDocument()
    await user.selectOptions(screen.getByRole('combobox'), 'living_standards')
    expect(screen.getByText('Build more Wheat Farms')).toBeInTheDocument()
    expect(screen.getByText('Fix the loss-making Shipyards')).toBeInTheDocument()
    expect(localStorage.getItem('vic3-advisor-economy-goal')).toBe('living_standards')
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
