import { useEffect, useRef } from 'react'
import { attachmentUrl } from '../api'
import type { Attachment } from '../types'

interface ComposerProps {
  value: string
  onChange: (value: string) => void
  onSend: () => void
  onStop: () => void
  busy: boolean
  placeholder: string
  modelLabel: string
  onModelChange: () => void
  /// Whether the chosen model can see images; when false the attach control is
  /// not offered at all rather than shown and rejected.
  vision: boolean
  attachments: Attachment[]
  onAddFiles: (files: FileList | null) => void
  onRemoveAttachment: (index: number) => void
}

// The message box: a soft rounded field that grows with what is typed, any
// images queued for sending, the model that will answer, and one round button
// that sends or stops.
export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  busy,
  placeholder,
  modelLabel,
  onModelChange,
  vision,
  attachments,
  onAddFiles,
  onRemoveAttachment,
}: ComposerProps) {
  const area = useRef<HTMLTextAreaElement>(null)
  const picker = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const el = area.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [value])

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !busy && (value.trim() || attachments.length > 0)) {
      e.preventDefault()
      onSend()
    }
  }

  const canSend = !busy && (value.trim() !== '' || attachments.length > 0)

  return (
    <div className="composer">
      {attachments.length > 0 && (
        <ul className="attachments">
          {attachments.map((attachment, index) => (
            <li key={`${attachment.mime}-${index}`} className="attachment">
              <img src={attachmentUrl(attachment)} alt="" />
              <button
                className="attachment-remove"
                onClick={() => onRemoveAttachment(index)}
                aria-label={`Remove image ${index + 1}`}
              >
                <svg viewBox="0 0 14 14" aria-hidden="true">
                  <path d="M3.5 3.5l7 7M10.5 3.5l-7 7" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
                </svg>
              </button>
            </li>
          ))}
        </ul>
      )}

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
        {vision && (
          <>
            <input
              ref={picker}
              type="file"
              accept="image/*"
              multiple
              hidden
              onChange={(e) => {
                onAddFiles(e.target.files)
                e.target.value = ''
              }}
            />
            <button
              className="attach"
              onClick={() => picker.current?.click()}
              aria-label="Attach an image"
              title="Attach an image"
            >
              <svg viewBox="0 0 18 18" aria-hidden="true">
                <path
                  d="M9 3.5v11M3.5 9h11"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.6"
                  strokeLinecap="round"
                />
              </svg>
            </button>
          </>
        )}
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
          <button className="send" onClick={onSend} disabled={!canSend} aria-label="Send message">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M8 13V3.5M8 3.5L4 7.5M8 3.5l4 4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}
