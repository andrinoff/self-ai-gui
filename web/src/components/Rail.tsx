import { ago } from '../api'
import type { Conversation } from '../types'

interface RailProps {
  conversations: Conversation[]
  activeId: number | null
  onSelect: (id: number) => void
  onNew: () => void
  onDelete: (id: number) => void
  memoryCount: number
  onOpenMemory: () => void
  model: string
  onOpenModels: () => void
  baseUrl: string
  hasKey: boolean
}

export function Rail({
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  memoryCount,
  onOpenMemory,
  model,
  onOpenModels,
  baseUrl,
  hasKey,
}: RailProps) {
  return (
    <aside className="rail">
      <header className="rail-head">
        <h1 className="brand">
          self<span className="brand-dot" />
        </h1>
        <p className="tagline">a chat that keeps what matters</p>
      </header>

      <button className="new-chat" onClick={onNew}>
        Start a new conversation
      </button>

      <nav className="threads" aria-label="Conversations">
        {conversations.length === 0 && <p className="rail-empty">Nothing here yet.</p>}
        {conversations.map((conversation) => (
          <div key={conversation.id} className={`thread ${conversation.id === activeId ? 'active' : ''}`}>
            <button className="thread-open" onClick={() => onSelect(conversation.id)}>
              <span className="thread-title">{conversation.title || 'New conversation'}</span>
              <span className="thread-meta">
                {ago(conversation.updated_at)}
                {conversation.message_count > 0 && ` · ${conversation.message_count} turns`}
              </span>
            </button>
            <button className="thread-delete" onClick={() => onDelete(conversation.id)} aria-label="Delete this conversation">
              ×
            </button>
          </div>
        ))}
      </nav>

      <footer className="rail-foot">
        <button className="memory-link" onClick={onOpenMemory}>
          <span className="memory-count">{memoryCount}</span> remembered
        </button>
        <button className="model-line" onClick={onOpenModels}>
          answering as <b>{model || '…'}</b>
        </button>
        <p className="rail-note" title={baseUrl}>
          {baseUrl.replace(/^https?:\/\//, '')}
          {!hasKey && ' · no key set'}
        </p>
      </footer>
    </aside>
  )
}
