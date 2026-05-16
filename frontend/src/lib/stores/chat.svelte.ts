// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { chatApi, streamChat } from '$lib/api/chat'
import type {
  Chat,
  ChatMessage,
  FileRef,
  OutgoingMessage,
  TemplateRef,
  WorkflowRef,
} from '$lib/types/chat'

/** Attachments selected in the composer for the next user message. */
export interface SendAttachments {
  files?: FileRef[]
  workflow?: WorkflowRef
  template?: TemplateRef
  /** When set on a brand-new chat, the chat is created under this project. */
  projectId?: string
}

function toOutgoing(m: ChatMessage): OutgoingMessage {
  return {
    role: m.role,
    content: m.content,
    ...(m.workflow ? { workflow: m.workflow } : {}),
    ...(m.template ? { template: m.template } : {}),
    ...(m.files && m.files.length
      ? { files: m.files.map((f) => ({ document_id: f.document_id })) }
      : {}),
  }
}

function createChatStore() {
  let chats = $state<Chat[]>([])
  let activeId = $state<string | null>(null)
  let messages = $state<ChatMessage[]>([])
  let streaming = $state<boolean>(false)
  let loadingChats = $state<boolean>(false)
  let loadingMessages = $state<boolean>(false)
  let error = $state<string | null>(null)
  let abortCtrl: AbortController | null = null

  async function refreshChats() {
    loadingChats = true
    try {
      const res = await chatApi.list()
      chats = res.chats
    } catch (e) {
      error = (e as Error).message
    } finally {
      loadingChats = false
    }
  }

  return {
    get chats() {
      return chats
    },
    get activeId() {
      return activeId
    },
    get messages() {
      return messages
    },
    get streaming() {
      return streaming
    },
    get loadingChats() {
      return loadingChats
    },
    get loadingMessages() {
      return loadingMessages
    },
    get error() {
      return error
    },

    refreshChats,

    /** Start a fresh, unsaved chat. */
    newChat() {
      activeId = null
      messages = []
      error = null
    },

    async selectChat(id: string) {
      activeId = id
      messages = []
      error = null
      loadingMessages = true
      try {
        const res = await chatApi.messages(id)
        messages = res.messages.map((m) => ({
          role: m.role === 'assistant' ? 'assistant' : 'user',
          content: m.content,
        }))
      } catch (e) {
        error = (e as Error).message
      } finally {
        loadingMessages = false
      }
    },

    async remove(id: string) {
      await chatApi.remove(id)
      chats = chats.filter((c) => c.id !== id)
      if (activeId === id) {
        activeId = null
        messages = []
      }
    },

    /** Send a user message and stream the assistant reply. */
    async send(text: string, attach: SendAttachments = {}) {
      if (streaming || !text.trim()) return
      error = null

      // A project attachment on a new chat needs a real chat row first.
      if (!activeId && attach.projectId) {
        try {
          const created = await chatApi.createRecord(attach.projectId)
          activeId = created.id
        } catch (e) {
          error = (e as Error).message
          return
        }
      }

      const userMsg: ChatMessage = {
        role: 'user',
        content: text.trim(),
        ...(attach.workflow ? { workflow: attach.workflow } : {}),
        ...(attach.template ? { template: attach.template } : {}),
        ...(attach.files && attach.files.length ? { files: attach.files } : {}),
      }
      messages = [...messages, userMsg]
      const outgoing = messages.map(toOutgoing)
      messages = [...messages, { role: 'assistant', content: '', streaming: true }]
      streaming = true

      abortCtrl = streamChat(
        { messages: outgoing, ...(activeId ? { chat_id: activeId } : {}) },
        {
          onChatId: (id) => {
            if (!activeId) activeId = id
          },
          onDelta: (delta) => {
            const last = messages[messages.length - 1]
            if (last && last.role === 'assistant') last.content += delta
          },
          onError: (msg) => {
            error = msg
          },
          onDone: () => {
            streaming = false
            abortCtrl = null
            const last = messages[messages.length - 1]
            if (last && last.role === 'assistant') last.streaming = false
            void refreshChats()
          },
        },
      )
    },

    /** Stop an in-flight generation. */
    abort() {
      abortCtrl?.abort()
      abortCtrl = null
      streaming = false
      const last = messages[messages.length - 1]
      if (last && last.role === 'assistant') last.streaming = false
    },
  }
}

export const chatStore = createChatStore()
