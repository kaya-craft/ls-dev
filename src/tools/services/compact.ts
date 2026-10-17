import { ref } from 'vue'

/** Table card width below which the table drops uptime and meters. */
export const COMPACT_BELOW = 640

/** Set by ServiceTable from its own observed width; read by the rows. */
export const tableCompact = ref(false)
