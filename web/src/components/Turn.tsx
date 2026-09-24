import { useEffect, useRef, useState } from 'react'
import { Markdown } from './Markdown'
import type { Message } from '../types'

interface TurnProps {
  message: Message
  streaming?: boolean
  error?: string
}

// One exchange: the person's question in a bubble on the right, the reply on
// the left with the model's reasoning folded away behind a disclosure.
export function Turn({ message, streaming, error }: TurnProps) {
  const reasoning = message.reasoning.trim()
  const [open, setOpen] = useState(false)
  const touched = useRef(false)
  const answered = useRef(false)

  // Follow along while the model is still thinking, then fold the trace away
  // the moment the answer starts, unless the reader opened it themselves.
  useEffect(() => {
    if (!reasoning) return
    if (touched.current) return
    if (message.content === '') {
      setOpen(true)
    } else if (!answered.current) {
      answered.current = true
      setOpen(false)
    }
  }, [reasoning, message.content])

  if (message.role === 'user') {
    return (
      <article className="turn user">
        <div className="bubble">{message.content}</div>
      </article>
    )
  }

  return (
    <article className="turn assistant">
      <span className="avatar" aria-hidden="true">
        <svg viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="4.5" fill="currentColor" />
        </svg>
      </span>
      <div className="turn-body">
        {reasoning && (
          <div className={`thinking ${open ? 'open' : ''}`}>
            <button
              className="thinking-toggle"
              onClick={() => {
                touched.current = true
                setOpen((value) => !value)
              }}
              aria-expanded={open}
            >
              <svg viewBox="0 0 12 12" aria-hidden="true" className="thinking-caret">
                <path d="M4.5 3l3 3-3 3" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span>{streaming && message.content === '' ? 'Thinking…' : 'Thoughts'}</span>
            </button>
            {open && <div className="thinking-body">{reasoning}</div>}
          </div>
        )}

        <div className="turn-text">
          <Markdown text={message.content} />
          {streaming && <span className="caret" aria-label="composing" />}
        </div>

        {error && <p className="turn-error">{error}</p>}
      </div>
    </article>
  )
}
