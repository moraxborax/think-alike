<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { ElMessage } from 'element-plus'
import { api, type KanbanNode, type KanbanResponse } from '../lib/api'

const route = useRoute()
const nodes = ref<KanbanNode[]>([])
const normalizedStress = ref(0)
const selectedNodeId = ref('')
const hoveredNodeId = ref('')
const frameRef = ref<HTMLDivElement | null>(null)
const svgRef = ref<SVGSVGElement | null>(null)
const viewport = reactive({ width: 1200, height: 760 })
const camera = reactive({ x: 0, y: 0, zoom: 1 })
const dragState = reactive({
  active: false,
  pointerId: 0,
  startX: 0,
  startY: 0,
  originX: 0,
  originY: 0
})

const suppressClickUntil = ref(0)
const moveAroundStoppedByUser = ref(false)
let resizeObserver: ResizeObserver | null = null
let moveAroundTimer: number | null = null
let moveAroundFrame: number | null = null

const animationState = reactive({
  active: false,
  fromX: 0,
  fromY: 0,
  fromZoom: 1,
  toX: 0,
  toY: 0,
  toZoom: 1,
  startedAt: 0,
  durationMs: 0
})

/** Keep in sync with `.graph-node-description` in main.css (`max-height: min(42vh, 320px)`). */
const GRAPH_NODE_DESCRIPTION_MAX_VH = 0.42
const GRAPH_NODE_DESCRIPTION_MAX_PX = 320

function graphPopoverEstimatedHeight(viewportHeight: number) {
  const descMax = Math.min(viewportHeight * GRAPH_NODE_DESCRIPTION_MAX_VH, GRAPH_NODE_DESCRIPTION_MAX_PX)
  const chrome =
    18 * 2 +
    40 +
    50 +
    14 +
    44
  return Math.ceil(descMax + chrome)
}

const isFullscreen = computed(() => route.query.fullscreen === 'true')
const shouldMoveAround = computed(() => route.query.movearound !== undefined)

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max)
}

function ageColor(ageHours: number) {
  const maxAgeHours = 24 * 30 * 6
  const ratio = clamp(ageHours / maxAgeHours, 0, 1)
  const hue = 220 - ratio * 220
  return `hsla(${hue}, 84%, 62%, 0.9)`
}

function labelOffset(index: number) {
  const lane = Math.floor(index / 5)
  const direction = index % 2 === 0 ? -1 : 1
  return {
    x: direction * Math.min(12 + lane * 6, 28),
    y: 12 + lane * 10
  }
}

const worldNodes = computed(() =>
  nodes.value.map((node, index) => {
    const selected = selectedNodeId.value === node.id
    const hovered = hoveredNodeId.value === node.id
    const offset = labelOffset(index)
    return {
      ...node,
      worldX: node.x * 34,
      worldY: node.y * 34,
      radius: selected ? 10.5 : hovered ? 8.5 : 6,
      labelWidth: selected ? 144 : 104,
      labelHeight: selected ? 40 : 28,
      labelX: offset.x,
      labelY: offset.y,
      tone: ageColor(node.age_hours)
    }
  })
)

const selectedNode = computed(() => worldNodes.value.find((node) => node.id === selectedNodeId.value) ?? null)

const visibleLabels = computed(() => {
  const zoom = camera.zoom
  return worldNodes.value.filter((node, index) => {
    if (selectedNodeId.value === node.id || hoveredNodeId.value === node.id) {
      return true
    }

    if (zoom >= 1.9) {
      return true
    }

    if (zoom >= 1.45) {
      return index % 2 === 0
    }

    return index % 4 === 0
  })
})

const selectedCardStyle = computed(() => {
  if (!selectedNode.value) {
    return undefined
  }

  const cardWidth = 320
  const nodeX = camera.x + selectedNode.value.worldX * camera.zoom
  const nodeY = camera.y + selectedNode.value.worldY * camera.zoom

  const maxTop = Math.max(86, viewport.height - graphPopoverEstimatedHeight(viewport.height))

  return {
    left: `${clamp(nodeX + 20, 14, viewport.width - cardWidth - 14)}px`,
    top: `${clamp(nodeY - 10, 86, maxTop)}px`
  }
})

const sceneTransform = computed(() => `translate(${camera.x} ${camera.y}) scale(${camera.zoom})`)

function updateViewport() {
  const rect = frameRef.value?.getBoundingClientRect()
  if (!rect) {
    return
  }

  viewport.width = Math.max(rect.width, 320)
  viewport.height = Math.max(rect.height, isFullscreen.value ? window.innerHeight : 480)
}

function applyCameraTarget(worldX: number, worldY: number, zoom = camera.zoom) {
  camera.zoom = zoom
  camera.x = viewport.width / 2 - worldX * zoom
  camera.y = viewport.height / 2 - worldY * zoom
}

function easeInOutCubic(value: number) {
  return value < 0.5 ? 4 * value * value * value : 1 - Math.pow(-2 * value + 2, 3) / 2
}

function stopCameraAnimation() {
  animationState.active = false
  if (moveAroundFrame !== null) {
    window.cancelAnimationFrame(moveAroundFrame)
    moveAroundFrame = null
  }
}

function animateCameraTo(worldX: number, worldY: number, zoom: number, durationMs: number) {
  stopCameraAnimation()
  animationState.active = true
  animationState.fromX = camera.x
  animationState.fromY = camera.y
  animationState.fromZoom = camera.zoom
  animationState.toZoom = zoom
  animationState.toX = viewport.width / 2 - worldX * zoom
  animationState.toY = viewport.height / 2 - worldY * zoom
  animationState.startedAt = performance.now()
  animationState.durationMs = durationMs

  const tick = (now: number) => {
    if (!animationState.active) {
      return
    }

    const progress = clamp((now - animationState.startedAt) / animationState.durationMs, 0, 1)
    const eased = easeInOutCubic(progress)
    camera.x = animationState.fromX + (animationState.toX - animationState.fromX) * eased
    camera.y = animationState.fromY + (animationState.toY - animationState.fromY) * eased
    camera.zoom = animationState.fromZoom + (animationState.toZoom - animationState.fromZoom) * eased

    if (progress < 1) {
      moveAroundFrame = window.requestAnimationFrame(tick)
      return
    }

    camera.x = animationState.toX
    camera.y = animationState.toY
    camera.zoom = animationState.toZoom
    animationState.active = false
    moveAroundFrame = null
  }

  moveAroundFrame = window.requestAnimationFrame(tick)
}

function fitView() {
  if (!worldNodes.value.length) {
    return
  }

  updateViewport()

  const minX = Math.min(...worldNodes.value.map((node) => node.worldX - 60))
  const maxX = Math.max(...worldNodes.value.map((node) => node.worldX + 60))
  const minY = Math.min(...worldNodes.value.map((node) => node.worldY - 60))
  const maxY = Math.max(...worldNodes.value.map((node) => node.worldY + 84))
  const zoom = clamp(
    Math.min((viewport.width - 30) / Math.max(maxX - minX, 220), (viewport.height - 30) / Math.max(maxY - minY, 220)),
    1.25,
    3.4
  )
  const centerX = (minX + maxX) / 2
  const centerY = (minY + maxY) / 2

  applyCameraTarget(centerX, centerY, zoom)
}

function centerOnSelected() {
  if (!selectedNode.value) {
    return
  }

  applyCameraTarget(selectedNode.value.worldX, selectedNode.value.worldY)
}

function randomNearbyNode(excludingId?: string) {
  const pool = worldNodes.value.filter((node) => node.id !== excludingId)
  if (!pool.length) {
    return null
  }

  return pool[Math.floor(Math.random() * pool.length)] ?? null
}

function scheduleMoveAround() {
  if (moveAroundTimer !== null) {
    window.clearTimeout(moveAroundTimer)
    moveAroundTimer = null
  }

  if (!shouldMoveAround.value || moveAroundStoppedByUser.value || !worldNodes.value.length) {
    return
  }

  const target = randomNearbyNode(selectedNodeId.value) ?? worldNodes.value[0]
  if (target) {
    selectedNodeId.value = target.id
    const targetZoom = clamp(camera.zoom * (0.92 + Math.random() * 0.22), 1.3, 3.6)
    animateCameraTo(target.worldX, target.worldY, targetZoom, 2600 + Math.random() * 1400)
  }

  moveAroundTimer = window.setTimeout(scheduleMoveAround, 3400 + Math.random() * 1800)
}

function stopMoveAround() {
  stopCameraAnimation()
  if (moveAroundTimer !== null) {
    window.clearTimeout(moveAroundTimer)
    moveAroundTimer = null
  }
}

function stopMoveAroundByUser() {
  moveAroundStoppedByUser.value = true
  stopMoveAround()
}

function selectNode(nodeId: string) {
  if (performance.now() < suppressClickUntil.value) {
    return
  }

  selectedNodeId.value = nodeId
  stopMoveAroundByUser()
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) {
    return
  }

  stopMoveAround()
  dragState.active = true
  dragState.pointerId = event.pointerId
  dragState.startX = event.clientX
  dragState.startY = event.clientY
  dragState.originX = camera.x
  dragState.originY = camera.y
  svgRef.value?.setPointerCapture(event.pointerId)
}

function onPointerMove(event: PointerEvent) {
  if (!dragState.active || dragState.pointerId !== event.pointerId) {
    return
  }

  const dx = event.clientX - dragState.startX
  const dy = event.clientY - dragState.startY
  camera.x = dragState.originX + dx
  camera.y = dragState.originY + dy

  if (Math.hypot(dx, dy) > 5) {
    suppressClickUntil.value = performance.now() + 120
  }
}

function onPointerUp(event: PointerEvent) {
  if (dragState.pointerId === event.pointerId) {
    svgRef.value?.releasePointerCapture(event.pointerId)
  }

  dragState.active = false
  if (shouldMoveAround.value) {
    scheduleMoveAround()
  }
}

function onWheel(event: WheelEvent) {
  stopMoveAround()
  updateViewport()

  const rect = svgRef.value?.getBoundingClientRect()
  if (!rect) {
    return
  }

  const pointerX = event.clientX - rect.left
  const pointerY = event.clientY - rect.top
  const worldX = (pointerX - camera.x) / camera.zoom
  const worldY = (pointerY - camera.y) / camera.zoom
  const nextZoom = clamp(camera.zoom * (event.deltaY < 0 ? 1.1 : 0.92), 0.7, 4.1)

  camera.zoom = nextZoom
  camera.x = pointerX - worldX * nextZoom
  camera.y = pointerY - worldY * nextZoom

  if (shouldMoveAround.value) {
    scheduleMoveAround()
  }
}

async function loadKanban() {
  try {
    const graphResponse = await api<KanbanResponse>('/api/kanban')
    nodes.value = graphResponse.nodes
    normalizedStress.value = graphResponse.normalized_stress
    selectedNodeId.value = graphResponse.nodes[0]?.id ?? ''
    await nextTick()
    fitView()
    scheduleMoveAround()
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'Failed to load kanban graph')
  }
}

watch(shouldMoveAround, (enabled) => {
  if (enabled) {
    moveAroundStoppedByUser.value = false
    scheduleMoveAround()
  } else {
    moveAroundStoppedByUser.value = false
    stopMoveAround()
  }
})

watch(isFullscreen, async () => {
  stopCameraAnimation()
  await nextTick()
  updateViewport()
  fitView()
})

onMounted(() => {
  resizeObserver = new ResizeObserver(() => {
    stopCameraAnimation()
    updateViewport()
    fitView()
  })

  if (frameRef.value) {
    resizeObserver.observe(frameRef.value)
  }

  loadKanban()
})

onBeforeUnmount(() => {
  stopMoveAround()
  resizeObserver?.disconnect()
})
</script>

<template>
  <div class="kanban-page" :class="{ fullscreen: isFullscreen }">
    <section class="panel cloud-panel" :class="{ 'cloud-panel-fullscreen': isFullscreen }">
      <div v-if="!isFullscreen" class="section-heading">
        <div>
          <p class="eyebrow">Live map</p>
          <h2>Thought cloud</h2>
        </div>
        <div class="kanban-summary-pills">
          <span class="pill">{{ nodes.length }} nodes</span>
          <span class="pill stress-pill">Stress {{ normalizedStress.toFixed(3) }}</span>
        </div>
      </div>

      <div ref="frameRef" class="cloud-frame" :class="{ 'cloud-frame-fullscreen': isFullscreen }">
        <div v-if="!isFullscreen" class="graph-toolbar graph-toolbar-floating">
          <div class="cloud-legend" aria-label="Publish time color scale">
            <span>New</span>
            <div class="cloud-legend-bar" />
            <span>6 months</span>
          </div>
          <span class="pill stress-pill">Stress {{ normalizedStress.toFixed(3) }}</span>
          <div class="graph-toolbar-actions">
            <button type="button" class="graph-button" @click="fitView">Reset view</button>
            <button v-if="selectedNode" type="button" class="graph-button" @click="centerOnSelected">Center selected</button>
          </div>
        </div>

        <svg
          ref="svgRef"
          class="cloud-svg"
          :class="{ 'cloud-svg-fullscreen': isFullscreen }"
          :viewBox="`0 0 ${viewport.width} ${viewport.height}`"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointerleave="onPointerUp"
          @wheel.prevent="onWheel"
        >
          <g :transform="sceneTransform">
            <g
              v-for="node in worldNodes"
              :key="node.id"
              class="cloud-node-group"
              @mouseenter="hoveredNodeId = node.id"
              @mouseleave="hoveredNodeId = ''"
            >
              <circle
                :cx="node.worldX"
                :cy="node.worldY"
                :r="node.radius"
                class="cloud-node"
                :class="{ active: selectedNodeId === node.id, hovered: hoveredNodeId === node.id }"
                :style="{ fill: node.tone }"
                @pointerdown.stop
                @click.stop="selectNode(node.id)"
              />

              <foreignObject
                :x="node.worldX - node.labelWidth / 2 + node.labelX"
                :y="node.worldY + node.radius + node.labelY"
                :width="node.labelWidth"
                :height="node.labelHeight"
              >
                <div
                  xmlns="http://www.w3.org/1999/xhtml"
                  class="cloud-label"
                  :class="{ active: selectedNodeId === node.id }"
                  :style="{ borderColor: `${node.tone}55` }"
                  @pointerdown.stop
                  @click.stop="selectNode(node.id)"
                >
                  {{ node.title }}
                </div>
              </foreignObject>
            </g>
          </g>
        </svg>

        <div v-if="selectedNode && selectedCardStyle" class="graph-node-popover cloud-node-popover" :style="selectedCardStyle">
          <button type="button" class="graph-popover-close" @click="selectedNodeId = ''">Close</button>
          <h3>{{ selectedNode.title }}</h3>
          <div class="graph-node-description" role="region" aria-label="Description">
            {{ selectedNode.description }}
          </div>
          <div class="graph-detail-meta">
            <span>{{ `Author @${selectedNode.author_login}` }}</span>
            <span>{{ Math.max(selectedNode.age_hours, 0) }}h ago</span>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>
