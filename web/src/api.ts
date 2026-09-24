import type { Attachment, Conversation, Memory, Message, ModelInfo, PublicConfig, StreamEvent } from './types'

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`
    try {
      const data = await res.json()
      if (data?.error) message = data.error
    } catch {
      /* keep the status line */
    }
    throw new Error(message)
  }
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

export const api = {
  config: () => request<PublicConfig>('GET', '/api/config'),
  models: () => request<ModelInfo[]>('GET', '/api/models'),
  conversations: () => request<Conversation[]>('GET', '/api/conversations'),
  newConversation: () => request<Conversation>('POST', '/api/conversations', {}),
  messages: (id: number) => request<Message[]>('GET', `/api/conversations/${id}/messages`),
  renameConversation: (id: number, patch: { title?: string; model?: string; systemPrompt?: string }) =>
    request<Conversation>('PATCH', `/api/conversations/${id}`, patch),
  deleteConversation: (id: number) => request<void>('DELETE', `/api/conversations/${id}`),
  memories: () => request<Memory[]>('GET', '/api/memories'),
  addMemory: (text: string, kind?: string, pinned?: boolean) =>
    request<Memory>('POST', '/api/memories', { text, kind, pinned }),
  patchMemory: (id: number, patch: { text?: string; kind?: string; pinned?: boolean; enabled?: boolean }) =>
    request<Memory>('PATCH', `/api/memories/${id}`, patch),
  deleteMemory: (id: number) => request<void>('DELETE', `/api/memories/${id}`),
}

/// Reads one SSE response from a POST, because the browser's EventSource
/// cannot send a body. The shape is fixed: start, delta*, done, memory?.
export async function streamMessage(
  conversationId: number,
  content: string,
  model: string,
  attachments: Attachment[],
  signal: AbortSignal,
  onEvent: (event: StreamEvent) => void,
): Promise<void> {
  const res = await fetch(`/api/conversations/${conversationId}/messages`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content, model, attachments }),
    signal,
  })
  if (!res.ok || !res.body) {
    let message = `${res.status} ${res.statusText}`
    try {
      const data = await res.json()
      if (data?.error) message = data.error
    } catch {
      /* keep the status line */
    }
    onEvent({ event: 'error', message })
    return
  }

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  const dispatch = (frame: string) => {
    let event = ''
    let data = ''
    for (const line of frame.split('\n')) {
      const [field, ...rest] = line.split(':')
      const value = rest.join(':')
      if (field === 'event') event = value.trim()
      else if (field === 'data') data += value.trimStart()
    }
    if (!data) return
    try {
      const parsed = JSON.parse(data)
      onEvent({ event: event || 'delta', ...parsed } as StreamEvent)
    } catch {
      /* a comment or heartbeat frame; nothing to do */
    }
  }

  while (true) {
    const { done, value } = await reader.read()
    if (value) buffer += decoder.decode(value, { stream: true })
    let boundary: number
    while ((boundary = buffer.indexOf('\n\n')) !== -1) {
      const frame = buffer.slice(0, boundary)
      buffer = buffer.slice(boundary + 2)
      dispatch(frame)
    }
    if (done) break
  }
  if (buffer.trim()) dispatch(buffer)
}

// --- small formatting helpers ---

/// The data URL for an attachment, which is what both an <img> and the model
/// expect. The prefix is added here so the stored base64 stays small.
export function attachmentUrl(attachment: Attachment): string {
  return `data:${attachment.mime};base64,${attachment.data}`
}

export function when(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  const sameDay = date.toDateString() === new Date().toDateString()
  const time = date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  if (sameDay) return time
  return `${date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} ${time}`
}

export function ago(iso: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000)
  if (seconds < 60) return 'just now'
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`
  return `${Math.floor(seconds / 86400)}d ago`
}
