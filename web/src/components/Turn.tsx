import { Markdown } from './Markdown'
import type { Memory, Message } from '../types'

interface TurnProps {
  message: Message
  memories: Map<number, Memory>
  streaming?: boolean
  learned?: Memory[]
  error?: string
  onRemember?: () => void
  onOpenMemory?: (id: number) => void
  remembering?: boolean
}

// One exchange in the transcript. Speaker attribution sits in the left margin
// the way a printed interview does, so the reply itself gets the whole measure.
export function Turn({
  message,
  memories,
  streaming,
  learned,
  error,
  onRemember,
  onOpenMemory,
  remembering,
}: TurnProps) {
  const used = message.memory_ids
    .map((id) => memories.get(id))
    .filter((memory): memory is Memory => Boolean(memory))

  return (
    <article className={`turn ${message.role}`}>
      <span className="speaker">{message.role === 'user' ? 'you' : 'self'}</span>
      <div className="turn-body">
        <div className="turn-text">
          <Markdown text={message.content} />
          {streaming && message.content === '' && <span className="caret" aria-label="composing" />}
          {streaming && message.content !== '' && <span className="caret" aria-label="composing" />}
        </div>

        {error && <p className="turn-error">{error}</p>}

        {message.role === 'assistant' && used.length > 0 && (
          <ul className="footnotes">
            {used.map((memory, i) => (
              <li key={memory.id}>
                <button className="footnote" onClick={() => onOpenMemory?.(memory.id)}>
                  <sup>{i + 1}</sup>
                  {memory.text}
                </button>
              </li>
            ))}
          </ul>
        )}

        {learned && learned.length > 0 && (
          <ul className="learned">
            {learned.map((memory) => (
              <li key={memory.id}>remembered: {memory.text}</li>
            ))}
          </ul>
        )}

        {message.role === 'assistant' && !streaming && message.content && onRemember && (
          <button
            className="remember"
            onClick={onRemember}
            disabled={remembering}
            title="Look through this conversation for things worth keeping"
          >
            {remembering ? 'reading the conversation…' : 'remember something'}
          </button>
        )}
      </div>
    </article>
  )
}
