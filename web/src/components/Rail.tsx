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
}

export function Rail({
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  memoryCount,
  onOpenMemory,
}: RailProps) {
  return (
    <aside className="rail">
      <div className="rail-head">
        <button className="new-chat" onClick={onNew}>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 3.5v9M3.5 8h9" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          New chat
        </button>
      </div>

      <nav className="threads" aria-label="Chats">
        {conversations.length === 0 && <p className="rail-empty">No chats yet.</p>}
        {conversations.map((conversation) => (
          <div key={conversation.id} className={`thread ${conversation.id === activeId ? 'active' : ''}`}>
            <button className="thread-open" onClick={() => onSelect(conversation.id)}>
              <span className="thread-title">{conversation.title || 'New chat'}</span>
              <span className="thread-meta">{ago(conversation.updated_at)}</span>
            </button>
            <button
              className="thread-delete"
              onClick={() => onDelete(conversation.id)}
              aria-label={`Delete ${conversation.title || 'this chat'}`}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
              </svg>
            </button>
          </div>
        ))}
      </nav>

      <div className="rail-foot">
        <button className="memory-link" onClick={onOpenMemory}>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 14s5-3.2 5-7A3 3 0 008 4.6 3 3 0 003 7c0 3.8 5 7 5 7z" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
          </svg>
          Memory
          <span className="memory-count">{memoryCount}</span>
        </button>
      </div>
    </aside>
  )
}
