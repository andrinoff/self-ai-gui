import { useEffect, useRef } from 'react'

interface ComposerProps {
  value: string
  onChange: (value: string) => void
  onSend: () => void
  onStop: () => void
  busy: boolean
  placeholder: string
  modelLabel: string
  onModelChange: () => void
}

// The message box: a soft rounded field that grows with what is typed, the
// model that will answer, and one round button that sends or stops.
export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  busy,
  placeholder,
  modelLabel,
  onModelChange,
}: ComposerProps) {
  const area = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const el = area.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [value])

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !busy && value.trim()) {
      e.preventDefault()
      onSend()
    }
  }

  return (
    <div className="composer">
      <textarea
        ref={area}
        className="composer-input"
        rows={1}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        aria-label="Message self"
      />
      <div className="composer-row">
        <button className="model-pick" onClick={onModelChange} title="Change the model">
          {modelLabel}
          <svg viewBox="0 0 12 12" aria-hidden="true" className="model-pick-caret">
            <path d="M3 5l3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
        {busy ? (
          <button className="send stop" onClick={onStop} aria-label="Stop the reply">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor" />
            </svg>
          </button>
        ) : (
          <button className="send" onClick={onSend} disabled={!value.trim()} aria-label="Send message">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M8 13V3.5M8 3.5L4 7.5M8 3.5l4 4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}
