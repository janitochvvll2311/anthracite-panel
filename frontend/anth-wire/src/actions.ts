/** Bodies a client sends and receives in common. */

/** Answer to `GET /api/hello`. */
export interface Hello {
  protocol_version: number
}

/** A mutation whose only output is a ready-to-print message. */
export interface Change {
  message: string
}

/** A file or directory was deleted. */
export interface FileDeleted {
  path: string
}

/** A file or directory moved to the trash. */
export interface FileTrashed {
  path: string
  id: string
}

/** A trash entry was restored. */
export interface FileRestored {
  id: string
  path: string
}

/** The trash was emptied. */
export interface TrashEmptied {
  removed: number
}

/** An archive was created. */
export interface ArchiveCreated {
  path: string
}

/** An archive was extracted. */
export interface ArchiveExtracted {
  path: string
  entries: string[]
}

/** A signal was delivered. */
export interface ProcessSignalled {
  pid: number
  signal: string
}

/** The daemon accepted a restart and is about to replace itself. */
export interface Restarting {
  pid: number
}

/** The daemon accepted a reboot and scheduled it. */
export interface Rebooting {
  delay_s: number
}

/** One page request, matching `anth_wire::PageRequest`. */
export interface PageRequest {
  limit?: number
  cursor?: string
}
