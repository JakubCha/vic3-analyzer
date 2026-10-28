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
    await user.click(screen.getByRole('tab', { name: /Economy/ }))
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
    await user.click(await screen.findByRole('tab', { name: /Economy/ }))
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

  it('shows the budget guard and the top items across domains in Now', async () => {
    const many: AdviceResult = {
      advice: Array.from({ length: 7 }, (_, i) => ({
        id: `budget:${i}`,
        domain: i % 2 ? ('war' as const) : ('economy' as const),
        priority: 1,
        title: `Item ${i}`,
        detail: '',
        why: i === 0 ? 'Debt grows with interest.' : undefined,
      })),
      limitations: [],
      budget: {
        risk: 'deficit',
        treasury: 486_803,
        weekly_balance: -7_231,
        debt: 0,
        credit_limit: 17_058_032,
        weeks_to_debt: 67.3,
        weeks_to_default: 2426,
        expenses: [{ group: 'military_goods', label: 'Military goods', amount: 24_769, share: 0.38, top: [] }],
        incomes: [],
      },
    }
    const api = {
      loaded_advice: vi.fn(async () => JSON.stringify(many)),
      loaded_checks: vi.fn(async () => '[]'),
    } as unknown as WasmApi
    render(<AdvisorPane api={api} analysisKey={1} tag="PRU" />)
    expect(await screen.findByText('Deficit')).toBeInTheDocument()
    expect(screen.getByLabelText('Budget')).toHaveTextContent(/−7\.2K\/week.*treasury lasts ~67 weeks/)
    expect(screen.getByText(/Military goods 24\.8K \(38%\)/)).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Now (5)' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByText('Item 4')).toBeInTheDocument()
    expect(screen.queryByText('Item 5')).not.toBeInTheDocument()
    expect(screen.getByText('Debt grows with interest.')).toBeInTheDocument()
  })

  it('puts a worthwhile budget verdict at the top of Now', async () => {
    const mixed: AdviceResult = {
      advice: [
        ...Array.from({ length: 6 }, (_, i) => ({ id: `growth:${i}`, domain: 'economy' as const, priority: 1, title: `Market ${i}`, detail: '' })),
        { id: 'budget:balance', domain: 'economy', priority: 2, title: 'Budget deficit', detail: '' },
      ],
      limitations: [],
    }
    const api = {
      loaded_advice: vi.fn(async () => JSON.stringify(mixed)),
      loaded_checks: vi.fn(async () => '[]'),
    } as unknown as WasmApi
    render(<AdvisorPane api={api} analysisKey={1} tag="PRU" />)
    const items = await screen.findAllByRole('listitem')
    expect(items[0]).toHaveTextContent('Budget deficit')
    expect(items).toHaveLength(5)
  })

  it('shows why now, how to do it, the market doctor and what followed advice did', async () => {
    const user = userEvent.setup()
    const result: AdviceResult = {
      advice: [
        {
          ...advice.advice[0],
          why_now: ['Iron is +40% against base price (rule: from +15%)'],
          steps: ['Open the Construction panel and choose Iron Mine.'],
        },
      ],
      limitations: [],
      market: [
        {
          good: 'tools',
          label: 'Tools',
          price_vs_base: 0.35,
          dear: true,
          producers: ['Tooling Workshops'],
          dear_inputs: ['Iron'],
          consumers: [],
          yours: [['Silesia', 'Tooling Workshops', 3, 1200]],
          advice: 'Tools is +35%, but your Tooling Workshops run on Iron, which is also dear.',
          fixes: ['Construction panel: build more Iron Mines in Silesia; it makes Iron.'],
        },
      ],
      outcomes: [
        {
          id: 'budget:military',
          title: 'Military upkeep is 40% of spending',
          before: { date: '1836.2.11', treasury: 300000 },
          after: { date: '1837.1.1', treasury: 120000 },
          summary: '1836.2.11 → 1837.1.1: military 40% → 28% of spending',
        },
      ],
    }
    const api = { loaded_advice: vi.fn(async () => JSON.stringify(result)), loaded_checks: vi.fn(async () => '[]') } as unknown as WasmApi
    render(<AdvisorPane api={api} analysisKey={1} tag="PRU" date="1840.1.1" />)
    expect(await screen.findByLabelText('Why now')).toHaveTextContent('Iron is +40%')
    expect(screen.getByText('How to do it')).toBeInTheDocument()
    expect(screen.getByLabelText('What followed advice did')).toHaveTextContent('military 40% → 28% of spending')
    await user.click(screen.getByRole('tab', { name: /Market \(1\)/ }))
    const market = screen.getByLabelText('Market')
    expect(market).toHaveTextContent('Tools +35%')
    expect(market).toHaveTextContent('Dear inputs (fix first): Iron')
    expect(screen.getByLabelText('Do this: Tools')).toHaveTextContent('build more Iron Mines in Silesia')
    expect(market).toHaveTextContent('Yours: Tooling Workshops in Silesia, 3 levels, 1.2K profit per level')
  })
})
