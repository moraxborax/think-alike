<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import GraphCanvas from '../components/GraphCanvas.vue'
import { api, type SimilarGraph, type Thought } from '../lib/api'

const myThoughts = ref<Thought[]>([])
const discoverableThoughts = ref<Thought[]>([])
const graph = ref<SimilarGraph | null>(null)
const publishing = ref(false)
const loadingGraph = ref(false)
const selectedThoughtId = ref('')
const searchTerm = ref('')
const form = ref({ title: '', description: '' })

const selectedThought = computed(
  () => discoverableThoughts.value.find((thought) => thought.id === selectedThoughtId.value) ?? null
)

const selectedOwnThought = computed(
  () => myThoughts.value.find((thought) => thought.id === selectedThoughtId.value) ?? null
)

const searchResults = computed(() => {
  const query = searchTerm.value.trim().toLowerCase()
  if (!query) {
    return discoverableThoughts.value
  }

  return discoverableThoughts.value.filter((thought) => {
    return (
      thought.title.toLowerCase().includes(query) ||
      thought.description.toLowerCase().includes(query) ||
      thought.author_login.toLowerCase().includes(query)
    )
  })
})

function formatAge(thought: Thought) {
  if (thought.age_hours < 1) {
    return 'just now'
  }

  if (thought.age_hours < 24) {
    return `${thought.age_hours}h ago`
  }

  const days = Math.floor(thought.age_hours / 24)
  if (days < 30) {
    return `${days}d ago`
  }

  const months = Math.floor(days / 30)
  return `${months}mo ago`
}

async function loadThoughtCollections(preferredThoughtId?: string) {
  const [mine, discoverable] = await Promise.all([
    api<Thought[]>('/api/thoughts'),
    api<Thought[]>('/api/thoughts/discover')
  ])

  myThoughts.value = mine
  discoverableThoughts.value = discoverable

  const nextThoughtId = preferredThoughtId ?? selectedThoughtId.value
  if (nextThoughtId && discoverableThoughts.value.some((thought) => thought.id === nextThoughtId)) {
    selectedThoughtId.value = nextThoughtId
  } else {
    selectedThoughtId.value = discoverableThoughts.value[0]?.id ?? ''
  }

  await loadGraph()
}

async function publishThought() {
  publishing.value = true
  try {
    const created = await api<Thought>('/api/thoughts', {
      method: 'POST',
      body: JSON.stringify(form.value)
    })
    form.value = { title: '', description: '' }
    ElMessage.success('Thought published')
    await loadThoughtCollections(created.id)
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'Failed to publish thought')
  } finally {
    publishing.value = false
  }
}

async function loadGraph() {
  if (!selectedThoughtId.value) {
    graph.value = null
    return
  }

  loadingGraph.value = true
  try {
    graph.value = await api<SimilarGraph>(`/api/thoughts/${selectedThoughtId.value}/similar`)
  } catch (error) {
    graph.value = null
    ElMessage.error(error instanceof Error ? error.message : 'Failed to load graph')
  } finally {
    loadingGraph.value = false
  }
}

async function exploreThought(thoughtId: string) {
  selectedThoughtId.value = thoughtId
  await loadGraph()
}

async function deleteThought(thought: Thought) {
  await ElMessageBox.confirm(`Delete "${thought.title}"?`, 'Delete thought', {
    type: 'warning',
    confirmButtonText: 'Delete',
    cancelButtonText: 'Cancel'
  })

  try {
    await api(`/api/thoughts/${thought.id}`, { method: 'DELETE' })
    ElMessage.success('Thought deleted')
    await loadThoughtCollections(thought.id === selectedThoughtId.value ? undefined : selectedThoughtId.value)
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'Failed to delete thought')
  }
}

async function refreshThought(thought: Thought) {
  try {
    const refreshed = await api<Thought>(`/api/thoughts/${thought.id}/refresh`, { method: 'POST' })
    ElMessage.success('Thought refreshed')
    await loadThoughtCollections(refreshed.id)
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'Failed to refresh thought')
  }
}

onMounted(async () => {
  try {
    await loadThoughtCollections()
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'Failed to load thoughts')
  }
})
</script>

<template>
  <div class="home-grid">
    <section class="panel">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Publish</p>
          <h2>Add a new thought</h2>
        </div>
        <span class="pill">30 per day</span>
      </div>

      <el-form class="thought-form" label-position="top" @submit.prevent="publishThought">
        <el-form-item label="Title">
          <el-input v-model="form.title" maxlength="120" show-word-limit placeholder="A sharp headline for your thought" />
        </el-form-item>
        <el-form-item label="Description">
          <el-input
            v-model="form.description"
            type="textarea"
            :rows="7"
            maxlength="5000"
            show-word-limit
            placeholder="Write the argument, intuition, or question you want to put into the vector space"
          />
        </el-form-item>
        <el-button type="primary" :loading="publishing" @click="publishThought">Publish thought</el-button>
      </el-form>
    </section>

    <section class="panel">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Find</p>
          <h2>Similarity graph</h2>
        </div>
        <span class="pill">{{ discoverableThoughts.length }} discoverable</span>
      </div>

      <el-input
        v-model="searchTerm"
        class="thought-search"
        clearable
        placeholder="Search all thoughts by title, description, or author"
      />

      <el-select v-model="selectedThoughtId" filterable placeholder="Choose any thought" class="thought-select" @change="loadGraph">
        <el-option v-for="thought in searchResults" :key="thought.id" :label="thought.title" :value="thought.id">
          <div class="thought-option thought-option-discover">
            <span>{{ thought.title }}</span>
            <small>@{{ thought.author_login }} · {{ formatAge(thought) }}</small>
          </div>
        </el-option>
      </el-select>

      <div v-if="selectedThought" class="selected-thought-summary">
        <h3>{{ selectedThought.title }}</h3>
        <p>{{ selectedThought.description }}</p>
        <div class="thought-card-actions compact-actions">
          <span class="thought-meta">@{{ selectedThought.author_login }} · {{ formatAge(selectedThought) }}</span>
          <div v-if="selectedOwnThought" class="thought-action-buttons">
            <button type="button" class="graph-button" @click="refreshThought(selectedOwnThought)">Refresh</button>
            <button type="button" class="graph-button danger-button" @click="deleteThought(selectedOwnThought)">Delete</button>
          </div>
        </div>
      </div>

      <GraphCanvas
        v-if="graph"
        :nodes="graph.nodes"
        :loading="loadingGraph"
        @explore="exploreThought"
      />
      <div v-else class="empty-state">Publish a thought to start exploring nearby ideas.</div>
    </section>

    <section class="panel timeline-panel">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Archive</p>
          <h2>Your recent thoughts</h2>
        </div>
      </div>

      <div v-if="myThoughts.length" class="thought-list">
        <article v-for="thought in myThoughts" :key="thought.id" class="thought-card">
          <h3>{{ thought.title }}</h3>
          <p>{{ thought.description }}</p>
          <div class="thought-card-actions">
            <div>
              <p class="thought-meta">@{{ thought.author_login }}</p>
              <time>{{ new Date(thought.created_at).toLocaleString() }}</time>
            </div>
            <div class="thought-action-buttons">
              <button type="button" class="graph-button" @click="exploreThought(thought.id)">Open graph</button>
              <button type="button" class="graph-button" @click="refreshThought(thought)">Refresh</button>
              <button type="button" class="graph-button danger-button" @click="deleteThought(thought)">Delete</button>
            </div>
          </div>
        </article>
      </div>
      <div v-else class="empty-state">No thoughts yet.</div>
    </section>
  </div>
</template>
