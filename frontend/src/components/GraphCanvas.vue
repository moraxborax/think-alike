<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { SimilarNode } from '../lib/api'

const props = defineProps<{
  nodes: SimilarNode[]
  loading: boolean
}>()

const emit = defineEmits<{
  explore: [nodeId: string]
}>()

const shellRef = ref<HTMLDivElement | null>(null)
const svgRef = ref<SVGSVGElement | null>(null)
const selectedNodeId = ref('')
const inspectorDismissed = ref(false)
const viewport = reactive({ width: 960, height: 520 })
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
let resizeObserver: ResizeObserver | null = null

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max)
}

function ageColor(ageHours: number) {
  const maxAgeHours = 24 * 30 * 6
  const ratio = clamp(ageHours / maxAgeHours, 0, 1)
  const hue = 220 - ratio * 220
  return `hsla(${hue}, 82%, 64%, 0.92)`
}

function labelOffset(index: number) {
  const row = Math.floor(index / 2)
  const direction = index % 2 === 0 ? -1 : 1
  return {
    x: direction * (24 + row * 14),
    y: 18 + row * 26
  }
}

const worldNodes = computed(() =>
  props.nodes.map((node, index) => {
    const offset = node.center ? { x: 0, y: 0 } : labelOffset(index - 1)
    return {
      ...node,
      worldX: node.x * 2.35,
      worldY: node.y * 2.35,
      radius: node.center ? 48 : clamp(16 + node.score * 18, 16, 28),
      labelWidth: node.center ? 188 : 132,
      labelHeight: node.center ? 64 : 44,
      labelX: offset.x,
      labelY: offset.y,
      tone: ageColor(node.age_hours)
    }
  })
)

const edges = computed(() => worldNodes.value.filter((node) => !node.center))

const selectedNode = computed(
  () => worldNodes.value.find((node) => node.id === selectedNodeId.value) ?? worldNodes.value[0] ?? null
)
const visibleInspectorNode = computed(() => (inspectorDismissed.value ? null : selectedNode.value))

const sceneTransform = computed(() => `translate(${camera.x} ${camera.y}) scale(${camera.zoom})`)

function updateViewport() {
  const rect = shellRef.value?.getBoundingClientRect()
  if (!rect) {
    return
  }

  viewport.width = Math.max(rect.width, 320)
  viewport.height = Math.max(rect.height, 420)
}

function fitView() {
  if (!worldNodes.value.length) {
    return
  }

  updateViewport()

  const minX = Math.min(...worldNodes.value.map((node) => node.worldX - node.radius - node.labelWidth / 2 - Math.abs(node.labelX)))
  const maxX = Math.max(...worldNodes.value.map((node) => node.worldX + node.radius + node.labelWidth / 2 + Math.abs(node.labelX)))
  const minY = Math.min(...worldNodes.value.map((node) => node.worldY - node.radius - 60))
  const maxY = Math.max(...worldNodes.value.map((node) => node.worldY + node.radius + node.labelHeight + node.labelY + 24))
  const contentWidth = Math.max(maxX - minX, 280)
  const contentHeight = Math.max(maxY - minY, 240)
  const zoom = clamp(
    Math.min((viewport.width - 100) / contentWidth, (viewport.height - 100) / contentHeight),
    0.7,
    1.8
  )
  const centerX = (minX + maxX) / 2
  const centerY = (minY + maxY) / 2

  camera.zoom = zoom
  camera.x = viewport.width / 2 - centerX * zoom
  camera.y = viewport.height / 2 - centerY * zoom
}

function centerOnNode(nodeId: string) {
  const node = worldNodes.value.find((entry) => entry.id === nodeId)
  if (!node) {
    return
  }

  camera.x = viewport.width / 2 - node.worldX * camera.zoom
  camera.y = viewport.height / 2 - node.worldY * camera.zoom
}

function selectNode(nodeId: string) {
  if (performance.now() < suppressClickUntil.value) {
    return
  }

  selectedNodeId.value = nodeId
  inspectorDismissed.value = false
}

function closeInspector() {
  inspectorDismissed.value = true
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) {
    return
  }

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
}

function onWheel(event: WheelEvent) {
  updateViewport()

  const rect = svgRef.value?.getBoundingClientRect()
  if (!rect) {
    return
  }

  const pointerX = event.clientX - rect.left
  const pointerY = event.clientY - rect.top
  const worldX = (pointerX - camera.x) / camera.zoom
  const worldY = (pointerY - camera.y) / camera.zoom
  const nextZoom = clamp(camera.zoom * (event.deltaY < 0 ? 1.1 : 0.92), 0.55, 2.8)

  camera.zoom = nextZoom
  camera.x = pointerX - worldX * nextZoom
  camera.y = pointerY - worldY * nextZoom
}

watch(
  () => props.nodes,
  async () => {
    selectedNodeId.value = props.nodes.find((node) => node.center)?.id ?? props.nodes[0]?.id ?? ''
    inspectorDismissed.value = false
    await nextTick()
    fitView()
  },
  { immediate: true }
)

onMounted(() => {
  updateViewport()
  resizeObserver = new ResizeObserver(() => {
    updateViewport()
    fitView()
  })

  if (shellRef.value) {
    resizeObserver.observe(shellRef.value)
  }
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
})
</script>

<template>
  <div ref="shellRef" class="graph-shell interactive-graph" :class="{ loading }">
    <div class="graph-toolbar">
      <span>Drag to pan, scroll to zoom, and inspect nearby people and ideas around your selected thought.</span>
      <div class="graph-toolbar-actions">
        <button type="button" class="graph-button" @click="fitView">Reset view</button>
        <button
          v-if="selectedNode"
          type="button"
          class="graph-button"
          @click="centerOnNode(selectedNode.id)"
        >
          Center selected
        </button>
      </div>
    </div>

    <svg
      ref="svgRef"
      class="graph-svg"
      :viewBox="`0 0 ${viewport.width} ${viewport.height}`"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointerleave="onPointerUp"
      @wheel.prevent="onWheel"
    >
      <g :transform="sceneTransform">
        <line
          v-for="node in edges"
          :key="`${node.id}-edge`"
          x1="0"
          y1="0"
          :x2="node.worldX"
          :y2="node.worldY"
          class="graph-edge"
        />

        <g
          v-for="node in worldNodes"
          :key="node.id"
          class="graph-node-group"
          :class="{ active: selectedNode?.id === node.id }"
        >
          <circle
            :cx="node.worldX"
            :cy="node.worldY"
            :r="node.radius"
            :class="node.center ? 'graph-node-center' : 'graph-node'"
            :style="{ fill: node.center ? undefined : node.tone }"
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
              class="graph-node-label"
              :class="{ center: node.center, active: selectedNode?.id === node.id }"
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

    <div v-if="visibleInspectorNode" class="graph-node-popover graph-inspector">
      <button type="button" class="graph-popover-close" @click="closeInspector">Close</button>
      <h3>{{ visibleInspectorNode.title }}</h3>
      <div class="graph-node-description" role="region" aria-label="Description">
        {{ visibleInspectorNode.description }}
      </div>
      <div class="graph-detail-meta">
        <span>@{{ visibleInspectorNode.author_login }}</span>
        <span v-if="visibleInspectorNode.center">Anchor thought</span>
        <span v-else>{{ `${Math.round(visibleInspectorNode.score * 100)}% similar` }}</span>
      </div>
    </div>

    <div v-if="loading" class="graph-loading">Refreshing graph...</div>
  </div>
</template>
