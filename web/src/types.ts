export interface Memory {
  id: number
  text: string
  kind: 'fact' | 'preference' | 'project' | 'person' | 'routine' | string
  source: 'user' | 'auto' | 'seed' | string
  pinned: boolean
  enabled: boolean
  created_at: string
  last_used_at: string
  use_count: number
}

export interface Conversation {
  id: number
  title: string
  model: string
  system_prompt: string
  message_count: number
  created_at: string
  updated_at: string
}

export interface Message {
  id: number
  conversation_id: number
  role: 'user' | 'assistant' | string
  content: string
  reasoning: string
  model: string
  memory_ids: number[]
  created_at: string
}

export interface ModelInfo {
  id: string
  label: string
}

export interface PublicConfig {
  baseUrl: string
  hasKey: boolean
  defaultModel: string
  pinnedModels: string[]
  memoryEnabled: boolean
  memoryEvery: number
  memoryBudget: number
  historyMessages: number
  person: string
}

export type StreamEvent =
  | { event: 'start'; messageId: number; model: string; memories: Memory[] }
  | { event: 'thinking'; text: string }
  | { event: 'delta'; text: string }
  | { event: 'done'; messageId: number; model: string }
  | { event: 'memory'; added: Memory[] }
  | { event: 'error'; message: string }
