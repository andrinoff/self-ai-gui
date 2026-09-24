import { useEffect, useState } from 'react'
import { ago } from '../api'
import type { Memory } from '../types'

interface MemoryDrawerProps {
  open: boolean
  memories: Memory[]
  autoEvery: number
  onClose: () => void
  onAdd: (text: string, kind: string) => void
  onPatch: (id: number, patch: { text?: string; pinned?: boolean; enabled?: boolean; kind?: string }) => void
  onDelete: (id: number) => void
}

// Everything the assistant has chosen to keep, and the controls to correct it.
// Corrections apply to future prompts only.
export function MemoryDrawer({
  open,
  memories,
  autoEvery,
  onClose,
  onAdd,
  onPatch,
  onDelete,
}: MemoryDrawerProps) {
  const [query, setQuery] = useState('')
  const [draft, setDraft] = useState('')
  const [editing, setEditing] = useState<number | null>(null)
  const [editText, setEditText] = useState('')

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, onClose])

  if (!open) return null

  const needle = query.trim().toLowerCase()
  const shown = needle
    ? memories.filter((memory) => memory.text.toLowerCase().includes(needle))
    : memories
  const pinned = shown.filter((m) => m.pinned)
  const rest = shown.filter((m) => !m.pinned)

  const row = (memory: Memory) => (
    <li key={memory.id} className={`note-row ${memory.pinned ? 'pinned' : ''} ${memory.enabled ? '' : 'muted'}`}>
      {editing === memory.id ? (
        <form
          className="note-edit"
          onSubmit={(e) => {
            e.preventDefault()
            if (editText.trim()) onPatch(memory.id, { text: editText.trim() })
            setEditing(null)
          }}
        >
          <textarea
            value={editText}
            onChange={(e) => setEditText(e.target.value)}
            rows={2}
            autoFocus
          />
          <span className="note-edit-actions">
            <button type="button" className="ghost" onClick={() => setEditing(null)}>
              Cancel
            </button>
            <button type="submit" className="solid">
              Save
            </button>
          </span>
        </form>
      ) : (
        <>
          <button
            className="note-pin"
            onClick={() => onPatch(memory.id, { pinned: !memory.pinned })}
            title={memory.pinned ? 'Pinned: always included' : 'Not pinned: included when relevant'}
            aria-pressed={memory.pinned}
          >
            {memory.pinned ? '◆' : '◇'}
          </button>
          <div className="note-main">
            <p className="note-text">{memory.text}</p>
            <p className="note-meta">
              <span className="kind">{memory.kind}</span>
              <span className="source">{memory.source === 'auto' ? 'learned' : 'written'}</span>
              {memory.use_count > 0 && <span className="uses">used {memory.use_count}×</span>}
              {memory.created_at && <span className="when">{ago(memory.created_at)}</span>}
              {!memory.enabled && <span className="off">off</span>}
            </p>
          </div>
          <span className="note-actions">
            <button
              className="ghost"
              onClick={() => onPatch(memory.id, { enabled: !memory.enabled })}
              title={memory.enabled ? 'Mute: never include in prompts' : 'Include again'}
            >
              {memory.enabled ? 'mute' : 'unmute'}
            </button>
            <button
              className="ghost"
              onClick={() => {
                setEditing(memory.id)
                setEditText(memory.text)
              }}
              title="Correct this note"
            >
              edit
            </button>
            <button className="ghost danger" onClick={() => onDelete(memory.id)} title="Forget this">
              forget
            </button>
          </span>
        </>
      )}
    </li>
  )

  return (
    <div className="scrim" onMouseDown={onClose}>
      <section
        className="drawer"
        role="dialog"
        aria-label="What the assistant remembers"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="drawer-head">
          <h2>Remembered</h2>
          <span className="drawer-note">
            {memories.length} notes · pinned notes are always in the prompt, the rest only when they
            are relevant
          </span>
          <button className="drawer-close" onClick={onClose} aria-label="Close">
            ×
          </button>
        </header>

        <form
          className="note-add"
          onSubmit={(e) => {
            e.preventDefault()
            if (draft.trim()) {
              onAdd(draft.trim(), 'fact')
              setDraft('')
            }
          }}
        >
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Tell me something to keep: I prefer…"
            aria-label="Add a note"
          />
          <button type="submit" className="solid" disabled={!draft.trim()}>
            Keep
          </button>
        </form>

        <input
          className="note-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search the notes…"
          aria-label="Search notes"
        />

        <div className="drawer-body">
          {memories.length === 0 && (
            <p className="drawer-empty">
              Nothing kept yet. Write a note above, or just talk: self reads the conversation every{' '}
              {autoEvery} turns and keeps whatever seems worth remembering.
            </p>
          )}
          {pinned.length > 0 && (
            <>
              <p className="group">always included</p>
              <ul className="notes">{pinned.map(row)}</ul>
            </>
          )}
          {rest.length > 0 && (
            <>
              <p className="group">when relevant</p>
              <ul className="notes">{rest.map(row)}</ul>
            </>
          )}
        </div>
      </section>
    </div>
  )
}
