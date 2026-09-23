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
  remember: () => void
  rememberBusy: boolean
}

// The writing area: a ruled line that grows, with the send control, the model
// the reply will come from, and a remember button for what matters.
export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  busy,
  placeholder,
  modelLabel,
  onModelChange,
  remember,
  rememberBusy,
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
        aria-label="Write a message"
      />
      <div className="composer-row">
        <button className="ghost" onClick={remember} disabled={rememberBusy} title="Save what this conversation taught me">
          {rememberBusy ? 'reading…' : 'remember'}
        </button>
        <span className="composer-hint">enter sends · shift-enter a new line</span>
        <button className="model-pick" onClick={onModelChange} title="Change the answering model">
          as {modelLabel}
        </button>
        {busy ? (
          <button className="send stop" onClick={onStop} aria-label="Stop the reply">
            Stop
          </button>
        ) : (
          <button className="send" onClick={onSend} disabled={!value.trim()} aria-label="Send">
            Send
          </button>
        )}
      </div>
    </div>
  )
}
