import { computed, ref } from 'vue'
import { useNavigation } from './navigation'

/** Window width below which the sidebar collapses to its icon rail. */
export const NARROW_BELOW = 960

/** Window width below which the detail pane becomes an overlay drawer. */
export const DRAWER_BELOW = 1280

const hasWindow = typeof window !== 'undefined'

/** True while the window is narrower than `NARROW_BELOW`. */
export const isNarrow = ref(hasWindow && window.innerWidth < NARROW_BELOW)

export const isDrawer = ref(hasWindow && window.innerWidth < DRAWER_BELOW)

// One shared resize listener for every breakpoint.
if (hasWindow) {
  window.addEventListener('resize', () => {
    isNarrow.value = window.innerWidth < NARROW_BELOW
    isDrawer.value = window.innerWidth < DRAWER_BELOW
  })
}

const { state: nav } = useNavigation()

/** True when the sidebar shows as an icon rail: window narrow or sidebar collapsed by the user. */
export const railShown = computed(() => isNarrow.value || nav.sidebarCollapsed)
