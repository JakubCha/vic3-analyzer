import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { HistoryPane } from './HistoryPane'
import type { RenderedChart } from './types'
import type { WasmApi } from './wasm'

const chart = (key: string, title: string, unit: RenderedChart['unit'], first: number, last: number): RenderedChart => ({
  key,
  title,
  unit,
  svg: '<svg viewBox="0 0 10 10"><polyline points="0,0 10,10"/></svg>',
  lines: [{ label: title, first, last }],
})

describe('HistoryPane', () => {
  afterEach(() => cleanup())

  it('shows each chart with its latest value and change', async () => {
    const charts = [chart('gdp', 'GDP', 'money', 9_720_602, 12_500_000), chart('literacy', 'Literacy', 'share', 0.3, 0.42)]
    const api = { loaded_history: vi.fn(async () => JSON.stringify(charts)) } as unknown as WasmApi
    const { container } = render(<HistoryPane api={api} analysisKey={1} />)
    expect(await screen.findByLabelText('GDP')).toHaveTextContent(/GDP 12\.5M \+29%/)
    expect(screen.getByLabelText('Literacy')).toHaveTextContent(/42% \+12 pp/)
    expect(container.querySelectorAll('svg polyline')).toHaveLength(2)
    expect(screen.getByText(/Budget history appears once/)).toBeInTheDocument()
  })
})
