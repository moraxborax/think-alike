export interface Viewer {
  github_id: number
  login: string
  avatar_url?: string | null
}

export interface Thought {
  id: string
  title: string
  description: string
  created_at: string
  age_hours: number
  author_github_id: number
  author_login: string
  author_avatar_url?: string | null
}

export interface SimilarNode {
  id: string
  title: string
  description: string
  age_hours: number
  author_github_id: number
  author_login: string
  author_avatar_url?: string | null
  score: number
  x: number
  y: number
  center: boolean
}

export interface SimilarGraph {
  center_id: string
  nodes: SimilarNode[]
}

export interface KanbanNode {
  id: string
  title: string
  description: string
  author_github_id: number
  author_login: string
  author_avatar_url?: string | null
  x: number
  y: number
  age_hours: number
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {})
    },
    ...init
  })

  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as { error?: string } | null
    throw new Error(payload?.error ?? `Request failed with ${response.status}`)
  }

  if (response.status === 204) {
    return undefined as T
  }

  return (await response.json()) as T
}
