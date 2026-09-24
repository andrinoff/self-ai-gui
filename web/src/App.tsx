import { useCallback, useEffect, useRef, useState } from 'react'
import { api, streamMessage } from './api'
import { Composer } from './components/Composer'
import { MemoryDrawer } from './components/MemoryDrawer'
import { Rail } from './components/Rail'
import { Turn } from './components/Turn'
import type { Conversation, Memory, Message, ModelInfo, PublicConfig } from './types'

export function App() {
  const [config, setConfig] = useState<PublicConfig | null>(null)
  const [models, setModels] = useState<ModelInfo[]>([])
  const [model, setModel] = useState('')
  const [modelMenu, setModelMenu] = useState(false)
  const [conversations, setConversations] = useState<Conversation[]>([])
  const [activeId, setActiveId] = useState<number | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [memories, setMemories] = useState<Memory[]>([])
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const [drawer, setDrawer] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const abort = useRef<AbortController | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)

  const show = useCallback((text: string) => {
    setNote(text)
    window.setTimeout(() => setNote((current) => (current === text ? null : current)), 4000)
  }, [])

  const refreshAll = useCallback(async () => {
    const [convs, mems] = await Promise.all([api.conversations(), api.memories()])
    setConversations(convs)
    setMemories(mems)
  }, [])

  useEffect(() => {
    Promise.all([api.config(), api.models()])
      .then(([cfg, m]) => {
        setConfig(cfg)
        setModels(m)
        setModel((prev) => prev || cfg.defaultModel)
      })
      .catch(() => setNote('The server did not answer. Is it running?'))
    refreshAll().catch(() => {})
  }, [refreshAll])

  const loadConversation = useCallback(async (id: number) => {
    setActiveId(id)
    setMessages(await api.messages(id))
  }, [])

  const active = conversations.find((c) => c.id === activeId) ?? null

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [messages, busy])

  const selectModel = async (id: string) => {
    setModel(id)
    setModelMenu(false)
    if (active) {
      await api.renameConversation(active.id, { model: id }).catch(() => {})
      refreshAll().catch(() => {})
    }
  }

  const stop = () => {
    abort.current?.abort()
    abort.current = null
    setBusy(false)
  }

  const send = async () => {
    const content = draft.trim()
    if (!content || busy) return

    let conversationId = activeId
    if (conversationId === null) {
      try {
        const conversation = await api.newConversation()
        conversationId = conversation.id
        setConversations((list) => [conversation, ...list])
        setActiveId(conversationId)
        setMessages([])
      } catch (e) {
        show(e instanceof Error ? e.message : 'Could not start a conversation')
        return
      }
    }

    const tempUserId = -Date.now()
    const tempAssistantId = tempUserId - 1
    setDraft('')
    setBusy(true)
    setError(null)
    setMessages((list) => [
      ...list,
      {
        id: tempUserId,
        conversation_id: conversationId!,
        role: 'user',
        content,
        reasoning: '',
        model: '',
        memory_ids: [],
        created_at: new Date().toISOString(),
      },
      {
        id: tempAssistantId,
        conversation_id: conversationId!,
        role: 'assistant',
        content: '',
        reasoning: '',
        model,
        memory_ids: [],
        created_at: new Date().toISOString(),
      },
    ])

    const ctrl = new AbortController()
    abort.current = ctrl
    try {
      await streamMessage(conversationId, content, model, ctrl.signal, (event) => {
        if (event.event === 'start') {
          setMessages((list) =>
            list.map((m) =>
              m.id === tempAssistantId
                ? { ...m, memory_ids: event.memories.map((x) => x.id), model: event.model }
                : m,
            ),
          )
        } else if (event.event === 'thinking') {
          setMessages((list) =>
            list.map((m) =>
              m.id === tempAssistantId ? { ...m, reasoning: m.reasoning + event.text } : m,
            ),
          )
        } else if (event.event === 'delta') {
          setMessages((list) =>
            list.map((m) => (m.id === tempAssistantId ? { ...m, content: m.content + event.text } : m)),
          )
        } else if (event.event === 'memory') {
          setMemories((list) => {
            const seen = new Set(list.map((m) => m.id))
            return [...list, ...event.added.filter((m) => !seen.has(m.id))]
          })
        } else if (event.event === 'error') {
          setError(event.message)
        }
      })
    } catch (e) {
      if (!(e instanceof DOMException && e.name === 'AbortError')) {
        setError(e instanceof Error ? e.message : 'The reply stopped arriving.')
      }
    } finally {
      setBusy(false)
      abort.current = null
      try {
        await refreshAll()
        if (conversationId !== null) {
          setMessages(await api.messages(conversationId))
        }
      } catch {
        /* the transcript already shows the stream */
      }
    }
  }

  const removeConversation = async (id: number) => {
    await api.deleteConversation(id).catch(() => {})
    if (activeId === id) {
      setActiveId(null)
      setMessages([])
    }
    refreshAll().catch(() => {})
  }

  const name = config?.person && config.person !== 'the user' ? config.person : ''
  const greeting = name ? `What can I help with, ${name}?` : 'What can I help with?'

  return (
    <div className="app">
      <Rail
        conversations={conversations}
        activeId={activeId}
        onSelect={(id) => {
          setError(null)
          loadConversation(id).catch(() => show('Could not load that conversation.'))
        }}
        onNew={() => {
          setActiveId(null)
          setMessages([])
          setError(null)
          setDraft('')
        }}
        onDelete={removeConversation}
        memoryCount={memories.filter((m) => m.enabled).length}
        onOpenMemory={() => setDrawer(true)}
      />

      <main className="chat" ref={scrollRef}>
        <div className="stack">
          {activeId === null && messages.length === 0 && (
            <div className="welcome">
              <h2 className="welcome-title">{greeting}</h2>
              <div className="suggestions">
                {['Explain something simply', 'Help me plan the week', 'What do you remember about me?'].map(
                  (s) => (
                    <button
                      key={s}
                      className="suggestion"
                      onClick={() => {
                        setDraft(s)
                        document.querySelector<HTMLTextAreaElement>('.composer-input')?.focus()
                      }}
                    >
                      {s}
                    </button>
                  ),
                )}
              </div>
            </div>
          )}

          {messages.map((message) => (
            <Turn
              key={message.id}
              message={message}
              streaming={busy && message.role === 'assistant' && message.id < 0}
              error={busy && message.role === 'assistant' && message.id < 0 ? error ?? undefined : undefined}
            />
          ))}
        </div>

        <div className="composer-wrap">
          {modelMenu && (
            <div className="model-menu" role="menu">
              <p className="model-menu-head">Model</p>
              {models.length === 0 && <p className="model-menu-empty">No models from the provider.</p>}
              {models.map((m) => (
                <button
                  key={m.id}
                  className={`model-option ${m.id === model ? 'active' : ''}`}
                  onClick={() => selectModel(m.id)}
                >
                  <span className="model-option-label">{m.label}</span>
                  <span className="model-option-id">{m.id}</span>
                </button>
              ))}
            </div>
          )}
          <Composer
            value={draft}
            onChange={setDraft}
            onSend={send}
            onStop={stop}
            busy={busy}
            placeholder={config?.hasKey === false ? 'Add SELF_API_KEY and restart' : 'Message self'}
            modelLabel={model || 'Select model…'}
            onModelChange={() => setModelMenu((open) => !open)}
          />
        </div>
      </main>

      <MemoryDrawer
        open={drawer}
        memories={memories}
        autoEvery={config?.memoryEvery ?? 2}
        onClose={() => setDrawer(false)}
        onAdd={async (text, kind) => {
          await api.addMemory(text, kind)
          refreshAll().catch(() => {})
        }}
        onPatch={async (id, patch) => {
          await api.patchMemory(id, patch)
          refreshAll().catch(() => {})
        }}
        onDelete={async (id) => {
          await api.deleteMemory(id)
          refreshAll().catch(() => {})
        }}
      />

      {note && (
        <div className="note-toast" role="status">
          {note}
        </div>
      )}
    </div>
  )
}
