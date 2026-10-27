import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { OverlayView } from './OverlayView.tsx'
import { createTauriApi } from './tauriApi.ts'

// Guest WDIO harness for Tauri e2e / docs screenshots (`vite build --mode desktop`).
// Sets window.__wdio_original_core__ / window.wdioTauri for @wdio/tauri-service.
if (import.meta.env.MODE === 'desktop' || import.meta.env.VITE_TAURI === '1') {
  await import('@wdio/tauri-plugin')
}

// The desktop app opens `index.html?overlay` as its always-on-top Now window.
const overlay = new URLSearchParams(window.location.search).has('overlay')

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {overlay ? (
      <OverlayView api={createTauriApi()} listen={(window as any).__TAURI__?.event?.listen} />
    ) : (
      <App />
    )}
  </StrictMode>,
)
