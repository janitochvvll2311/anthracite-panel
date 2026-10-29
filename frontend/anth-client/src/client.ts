/**
 * The flat client, mirroring `anth_client::Client` method for method.
 *
 * Rust uses `snake_case`; TypeScript uses `camelCase`. Every other part of the
 * shape is the same: one object, typed DTO returns, the same error fields, and
 * a `next()` stream. The browser-only surfaces (`auth`, `settings`, `apps`)
 * hang off the same object; the CLI does not need them.
 */

import type {
  AccountCreate,
  AccountDelete,
  AccountGroupCreate,
  AccountGroupDelete,
  AccountSetPassword,
  AccountSnapshot,
  ArchiveCreated,
  ArchiveExtracted,
  ArchiveFormat,
  ArchiveListing,
  Change,
  FileChunk,
  FileDeleted,
  FilePage,
  FileRestored,
  FileTrashed,
  FileWritten,
  FirewallAction,
  FirewallChain,
  FirewallProtocol,
  FirewallSnapshot,
  Hello,
  HostInfo,
  NetworkCreateRequest,
  NetworkEdit,
  NetworkSnapshot,
  PageRequest,
  ProcessSignal,
  ProcessSignalled,
  ProcessSnapshot,
  ProcessStarted,
  Rebooting,
  Restarting,
  TerminalSize,
  TrashEmptied,
  TrashListing,
  VolumeFormatRequest,
  VolumeMountRequest,
  VolumePartitionCreate,
  VolumePartitionDelete,
  VolumePartitionResize,
  VolumeSnapshot,
  VolumeUnmountRequest,
} from 'anth-wire'

import { HttpAccountClient } from './accounts-client'
import { HttpAppsClient } from './apps-client'
import { HttpAuthClient } from './auth-client'
import { HttpFilesClient } from './files-client'
import { HttpFirewallClient } from './firewall-client'
import { HttpHostInfoClient } from './host-info-client'
import {
  HttpTransport,
  type HttpTransportOptions,
} from './http'
import { HttpNetworkClient } from './network-client'
import { HttpProcessesClient } from './processes-client'
import { HttpSettingsClient } from './settings-client'
import { HttpStorageClient } from './storage-client'
import { TerminalSession } from './terminal-session'

export class AnthClient {
  readonly transport: HttpTransport
  /** Browser login and registration; the CLI uses peer credentials. */
  readonly auth: HttpAuthClient
  /** Panel settings; the CLI edits the file with `config set`. */
  readonly settings: HttpSettingsClient
  /** App catalogue; the CLI manages bundles locally. */
  readonly apps: HttpAppsClient

  private readonly host: HttpHostInfoClient
  private readonly networks: HttpNetworkClient
  private readonly firewalls: HttpFirewallClient
  private readonly storage: HttpStorageClient
  private readonly accountClient: HttpAccountClient
  private readonly processClient: HttpProcessesClient
  private readonly files: HttpFilesClient

  constructor(options: HttpTransport | HttpTransportOptions = {}) {
    this.transport =
      options instanceof HttpTransport ? options : new HttpTransport(options)
    this.host = new HttpHostInfoClient(this.transport)
    this.networks = new HttpNetworkClient(this.transport)
    this.firewalls = new HttpFirewallClient(this.transport)
    this.storage = new HttpStorageClient(this.transport)
    this.accountClient = new HttpAccountClient(this.transport)
    this.processClient = new HttpProcessesClient(this.transport)
    this.files = new HttpFilesClient(this.transport)
    this.auth = new HttpAuthClient(this.transport)
    this.settings = new HttpSettingsClient(this.transport)
    this.apps = new HttpAppsClient(this.transport)
  }

  // ---- handshake -------------------------------------------------------

  hello(): Promise<Hello> {
    return this.transport.json<Hello>('GET', '/api/hello')
  }

  // ---- host ------------------------------------------------------------

  hostInfo(): Promise<HostInfo> {
    return this.host.get()
  }

  watchHostInfo(intervalMs: number) {
    return this.host.stream(intervalMs)
  }

  // ---- network ---------------------------------------------------------

  network(): Promise<NetworkSnapshot> {
    return this.networks.get()
  }

  watchNetwork(intervalMs: number) {
    return this.networks.stream(intervalMs)
  }

  networkSetState(name: string, up: boolean): Promise<Change> {
    return this.networks.setState(name, up)
  }

  networkCreate(request: NetworkCreateRequest): Promise<Change> {
    return this.networks.create(request)
  }

  networkEdit(request: NetworkEdit): Promise<Change> {
    return this.networks.edit(request)
  }

  networkRename(name: string, newName: string): Promise<Change> {
    return this.networks.rename(name, newName)
  }

  networkDelete(name: string): Promise<Change> {
    return this.networks.remove(name)
  }

  // ---- firewall --------------------------------------------------------

  firewall(): Promise<FirewallSnapshot> {
    return this.firewalls.get()
  }

  firewallAdd(
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<Change> {
    return this.firewalls.add(chain, action, protocol, port, comment)
  }

  firewallUpdate(
    handle: number,
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<Change> {
    return this.firewalls.update(
      handle,
      chain,
      action,
      protocol,
      port,
      comment,
    )
  }

  firewallDelete(handle: number): Promise<Change> {
    return this.firewalls.remove(handle)
  }

  firewallFlush(chain: FirewallChain): Promise<Change> {
    return this.firewalls.flush(chain)
  }

  firewallPolicy(
    chain: FirewallChain,
    policy: FirewallAction,
  ): Promise<Change> {
    return this.firewalls.setPolicy(chain, policy)
  }

  // ---- storage ---------------------------------------------------------

  volumes(): Promise<VolumeSnapshot> {
    return this.storage.get()
  }

  volumeMount(request: VolumeMountRequest): Promise<Change> {
    return this.storage.mount(request)
  }

  volumeUnmount(request: VolumeUnmountRequest): Promise<Change> {
    return this.storage.unmount(request)
  }

  volumePartitionCreate(request: VolumePartitionCreate): Promise<Change> {
    return this.storage.createPartition(request)
  }

  volumePartitionResize(request: VolumePartitionResize): Promise<Change> {
    return this.storage.resizePartition(request)
  }

  volumePartitionDelete(request: VolumePartitionDelete): Promise<Change> {
    return this.storage.deletePartition(request)
  }

  volumeFormat(request: VolumeFormatRequest): Promise<Change> {
    return this.storage.formatPartition(request)
  }

  // ---- accounts --------------------------------------------------------

  accounts(): Promise<AccountSnapshot> {
    return this.accountClient.get()
  }

  accountCreate(request: AccountCreate): Promise<Change> {
    return this.accountClient.createUser(request)
  }

  accountDelete(request: AccountDelete): Promise<Change> {
    return this.accountClient.deleteUser(request)
  }

  accountPassword(request: AccountSetPassword): Promise<Change> {
    return this.accountClient.setPassword(request)
  }

  accountGroupCreate(request: AccountGroupCreate): Promise<Change> {
    return this.accountClient.createGroup(request)
  }

  accountGroupDelete(request: AccountGroupDelete): Promise<Change> {
    return this.accountClient.deleteGroup(request)
  }

  // ---- processes -------------------------------------------------------

  processes(
    limit?: number,
    cursor?: string,
  ): Promise<ProcessSnapshot> {
    return this.processClient.list(limit, cursor)
  }

  watchProcesses(intervalMs: number, limit?: number) {
    return this.processClient.stream(intervalMs, limit)
  }

  processSignal(
    pid: number,
    startTicks: number,
    signal: ProcessSignal,
    group = false,
  ): Promise<ProcessSignalled> {
    return this.processClient.signal(pid, startTicks, signal, group)
  }

  processSpawn(
    program: string,
    args: string[],
    cwd?: string,
  ): Promise<ProcessStarted> {
    return this.processClient.spawn(program, args, cwd)
  }

  // ---- files -----------------------------------------------------------

  filesList(
    path: string,
    request: PageRequest = {},
  ): Promise<FilePage> {
    return this.files.list(path, request.cursor, request.limit)
  }

  filesRead(
    path: string,
    offset = 0,
    maxBytes?: number,
  ): Promise<FileChunk> {
    return this.files.read(path, offset, maxBytes)
  }

  filesDownload(path: string): Promise<Blob> {
    return this.files.download(path)
  }

  filesDelete(path: string): Promise<FileDeleted> {
    return this.files.delete(path)
  }

  filesWrite(
    path: string,
    content: string,
    create: boolean,
  ): Promise<FileWritten> {
    return this.files.write(path, content, create)
  }

  filesUpload(
    path: string,
    contentBase64: string,
    create: boolean,
  ): Promise<FileWritten> {
    return this.files.upload(path, contentBase64, create)
  }

  filesTrash(path: string): Promise<FileTrashed> {
    return this.files.trash(path)
  }

  trashList(): Promise<TrashListing> {
    return this.files.trashList()
  }

  trashRestore(id: string, overwrite = false): Promise<FileRestored> {
    return this.files.restore(id, overwrite)
  }

  trashEmpty(): Promise<TrashEmptied> {
    return this.files.emptyTrash()
  }

  archiveCreate(
    path: string,
    destination?: string,
    format?: ArchiveFormat,
  ): Promise<ArchiveCreated> {
    return this.files.archive(path, destination, format)
  }

  archiveList(path: string): Promise<ArchiveListing> {
    return this.files.archiveList(path)
  }

  archiveExtract(
    path: string,
    into?: string,
  ): Promise<ArchiveExtracted> {
    return this.files.extractArchive(path, into)
  }

  // ---- lifecycle -------------------------------------------------------

  restart(): Promise<Restarting> {
    return this.settings.restart()
  }

  reboot(delaySeconds: number): Promise<Rebooting> {
    return this.settings.reboot(delaySeconds)
  }

  // ---- terminal --------------------------------------------------------

  terminal(size: TerminalSize): Promise<TerminalSession> {
    const origin =
      this.transport.baseUrl ||
      globalThis.location?.origin ||
      'http://localhost'
    const url = new URL('/api/terminal', origin)
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
    url.searchParams.set('cols', String(size.cols))
    url.searchParams.set('rows', String(size.rows))
    return TerminalSession.open(url.toString())
  }

  // ---- raw -------------------------------------------------------------

  /** Sends one request without decoding the body or mapping its status. */
  requestRaw(
    method: string,
    path: string,
    body?: unknown,
    headers?: HeadersInit,
  ): Promise<Response> {
    return this.transport.request(method, path, body, { headers })
  }
}
