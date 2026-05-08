export type LocusClientOptions = {
  traceLatency?: boolean;
};

export type LinkAddedSignal = { type: 'link-added'; source: NodeId; relation: Relation; target: NodeId };
export type LinkRemovedSignal = { type: 'link-removed'; source: NodeId; relation: Relation; target: NodeId };
export type LinkSetSignal = { type: 'link-set'; source: NodeId; relation: Relation; oldTargets: NodeId[]; target: NodeId };
export type PropertyChangedSignal = { type: 'property-changed'; subject: NodeId; key: PropertyKey; value: string };
export type PropertyRemovedSignal = { type: 'property-removed'; subject: NodeId; key: PropertyKey };
export type ResolveChangedSignal = { type: 'resolve-changed'; source: NodeId; path: Relation[]; target: OptionalNodeId };
export type WatchPropertiesUpdatedSignal = { type: 'watch-properties-updated'; changed: Record<string, string>; removed: string[] };

const BUS_NAME = 'io.github.Locus';
const ROOT_PATH = '/io/github/Locus';
const GRAPH_READ_IFACE = 'io.github.Locus.Graph.Read';
const GRAPH_WRITE_IFACE = 'io.github.Locus.Graph.Write';
const GRAPH_RESOLVE_IFACE = 'io.github.Locus.Graph.Resolve';
const WATCH_IFACE = 'io.github.Locus.Watch';
const PROPERTIES_IFACE = 'org.freedesktop.DBus.Properties';
const NONE = '';

function none(value: string): OptionalNodeId {
  return value.length === 0 ? null : value;
}

function unpackVariant(value: any): any {
  return value && typeof value.deepUnpack === 'function' ? value.deepUnpack() : value;
}

export function samePath(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((part, index) => part === right[index]);
}

export function path(name: NamedPath): { from: NodeId; path: Relation[]; many: boolean } {
  const spec = locusSchema.paths[name];
  return { from: spec.from, path: [...spec.path], many: spec.many };
}

function nowMs(): number {
  return GLib.get_monotonic_time() / 1000;
}

export class LocusWatch {
  constructor(private readonly client: LocusDbusClient, readonly objectPath: string) {}

  async source(): Promise<NodeId> {
    return await this.client.watchProperty(this.objectPath, 'Source') as NodeId;
  }

  async path(): Promise<Relation[]> {
    return await this.client.watchProperty(this.objectPath, 'Path') as Relation[];
  }

  async target(): Promise<OptionalNodeId> {
    return none(await this.client.watchProperty(this.objectPath, 'Target') as string);
  }

  async properties(): Promise<Record<string, string>> {
    return await this.client.watchProperty(this.objectPath, 'Properties') as Record<string, string>;
  }

  async property(key: string): Promise<string> {
    return (await this.properties())[key] ?? '';
  }

  onTargetChanged(handler: (target: OptionalNodeId) => void): Unsubscribe {
    return this.client.onWatchPropertiesChanged(this.objectPath, changed => {
      if (!Object.prototype.hasOwnProperty.call(changed, 'Target')) return;
      handler(none(String(unpackVariant(changed.Target) ?? '')));
    });
  }

  onPropertiesUpdated(handler: (signal: WatchPropertiesUpdatedSignal) => void): Unsubscribe {
    return this.client.onWatchPropertiesUpdated(this.objectPath, handler);
  }

  onPropertyUpdated(key: string, handler: (value: string) => void): Unsubscribe {
    return this.onPropertiesUpdated(signal => {
      if (Object.prototype.hasOwnProperty.call(signal.changed, key)) handler(signal.changed[key] ?? '');
      else if (signal.removed.includes(key)) handler('');
    });
  }

  close(): Promise<void> {
    return this.client.closeWatch(this.objectPath);
  }
}

export class LocusDbusClient {
  private readonly traceLatency: boolean;

  constructor(options: LocusClientOptions = {}) {
    this.traceLatency = options.traceLatency ?? false;
  }

  setLink(source: NodeId, relation: Relation, target: NodeId): Promise<void> {
    return this.callWrite('SetLink', new GLib.Variant('(sss)', [source, relation, target]), null, () => undefined);
  }

  removeLink(source: NodeId, relation: Relation, target: NodeId): Promise<void> {
    return this.callWrite('RemoveLink', new GLib.Variant('(sss)', [source, relation, target]), null, () => undefined);
  }

  removeLinks(source: NodeId, relation: Relation): Promise<void> {
    return this.callWrite('RemoveLinks', new GLib.Variant('(ss)', [source, relation]), null, () => undefined);
  }

  deleteNode(subject: NodeId): Promise<void> {
    return this.callWrite('DeleteNode', new GLib.Variant('(s)', [subject]), null, () => undefined);
  }

  targets(source: NodeId, relation: Relation): Promise<NodeId[]> {
    return this.callRead('GetTargets', new GLib.Variant('(ss)', [source, relation]), '(as)', ([targets]) => targets as NodeId[]);
  }

  sources(target: NodeId, relation: Relation): Promise<NodeId[]> {
    return this.callRead('GetSources', new GLib.Variant('(ss)', [target, relation]), '(as)', ([sources]) => sources as NodeId[]);
  }

  setProperty(subject: NodeId, key: PropertyKey, value: string): Promise<void> {
    return this.callWrite('SetProperty', new GLib.Variant('(sss)', [subject, key, value]), null, () => undefined);
  }

  async property(subject: NodeId, key: PropertyKey): Promise<OptionalNodeId> {
    return none(await this.callRead('GetProperty', new GLib.Variant('(ss)', [subject, key]), '(s)', ([value]) => value as string));
  }

  properties(subject: NodeId): Promise<Record<string, string>> {
    return this.callRead('GetProperties', new GLib.Variant('(s)', [subject]), '(a{ss})', ([properties]) => properties as Record<string, string>);
  }

  findSubjects(key: PropertyKey, value: string): Promise<NodeId[]> {
    return this.callRead('FindSubjects', new GLib.Variant('(ss)', [key, value]), '(as)', ([subjects]) => subjects as NodeId[]);
  }

  async resolve(source: NodeId, relations: Relation[]): Promise<OptionalNodeId> {
    return none(await this.callResolve('Resolve', new GLib.Variant('(sas)', [source, relations]), '(s)', ([target]) => target as string));
  }

  resolveAll(source: NodeId, relations: Relation[]): Promise<NodeId[]> {
    return this.callResolve('ResolveAll', new GLib.Variant('(sas)', [source, relations]), '(as)', ([targets]) => targets as NodeId[]);
  }

  resolvePath(name: NamedPath, source?: NodeId): Promise<OptionalNodeId> {
    const spec = path(name);
    return this.resolve(source ?? spec.from, spec.path);
  }

  resolveAllPath(name: NamedPath, source?: NodeId): Promise<NodeId[]> {
    const spec = path(name);
    return this.resolveAll(source ?? spec.from, spec.path);
  }

  async subscribeResolve(source: NodeId, relations: Relation[]): Promise<OptionalNodeId> {
    return none(await this.callResolve('SubscribeResolve', new GLib.Variant('(sas)', [source, relations]), '(s)', ([target]) => target as string));
  }

  subscribePath(name: NamedPath, source?: NodeId): Promise<OptionalNodeId> {
    const spec = path(name);
    return this.subscribeResolve(source ?? spec.from, spec.path);
  }

  async watchNode(source: NodeId, relations: Relation[]): Promise<LocusWatch> {
    const objectPath = await this.callResolve('WatchNode', new GLib.Variant('(sas)', [source, relations]), '(o)', ([objectPath]) => objectPath as string);
    return new LocusWatch(this, objectPath);
  }

  watchPath(name: NamedPath, source?: NodeId): Promise<LocusWatch> {
    const spec = path(name);
    return this.watchNode(source ?? spec.from, spec.path);
  }

  onLinkAdded(handler: (signal: LinkAddedSignal) => void): Unsubscribe {
    return this.subscribeSignal(GRAPH_WRITE_IFACE, 'LinkAdded', null, null, params => {
      const [source, relation, target] = params.deepUnpack() as [NodeId, Relation, NodeId];
      handler({ type: 'link-added', source, relation, target });
    });
  }

  onLinkRemoved(handler: (signal: LinkRemovedSignal) => void): Unsubscribe {
    return this.subscribeSignal(GRAPH_WRITE_IFACE, 'LinkRemoved', null, null, params => {
      const [source, relation, target] = params.deepUnpack() as [NodeId, Relation, NodeId];
      handler({ type: 'link-removed', source, relation, target });
    });
  }

  onLinkSet(handler: (signal: LinkSetSignal) => void): Unsubscribe {
    return this.subscribeSignal(GRAPH_WRITE_IFACE, 'LinkSet', null, null, params => {
      const [source, relation, oldTargets, target] = params.deepUnpack() as [NodeId, Relation, NodeId[], NodeId];
      handler({ type: 'link-set', source, relation, oldTargets, target });
    });
  }

  onPropertyChanged(handler: (signal: PropertyChangedSignal) => void): Unsubscribe {
    return this.subscribeSignal(GRAPH_WRITE_IFACE, 'PropertyChanged', null, null, params => {
      const [subject, key, value] = params.deepUnpack() as [NodeId, PropertyKey, string];
      handler({ type: 'property-changed', subject, key, value });
    });
  }

  onPropertyRemoved(handler: (signal: PropertyRemovedSignal) => void): Unsubscribe {
    return this.subscribeSignal(GRAPH_WRITE_IFACE, 'PropertyRemoved', null, null, params => {
      const [subject, key] = params.deepUnpack() as [NodeId, PropertyKey];
      handler({ type: 'property-removed', subject, key });
    });
  }

  onResolveChanged(handler: (signal: ResolveChangedSignal) => void, source?: NodeId): Unsubscribe {
    return this.subscribeSignal(GRAPH_RESOLVE_IFACE, 'ResolveChanged', null, source ?? null, params => {
      const [changedSource, changedPath, target] = params.deepUnpack() as [NodeId, Relation[], string];
      handler({ type: 'resolve-changed', source: changedSource, path: changedPath, target: none(target) });
    });
  }

  watchProperty(objectPath: string, property: 'Source' | 'Path' | 'Target' | 'Properties'): Promise<string | string[] | Record<string, string>> {
    return this.callOn(objectPath, PROPERTIES_IFACE, 'Get', new GLib.Variant('(ss)', [WATCH_IFACE, property]), '(v)', ([value]) => unpackVariant(value));
  }

  onWatchPropertiesChanged(objectPath: string, handler: (changed: Record<string, any>) => void): Unsubscribe {
    return this.subscribeSignal(PROPERTIES_IFACE, 'PropertiesChanged', objectPath, WATCH_IFACE, params => {
      const [iface, changed] = params.deepUnpack() as [string, Record<string, any>, string[]];
      if (iface === WATCH_IFACE) handler(changed);
    });
  }

  onWatchPropertiesUpdated(objectPath: string, handler: (signal: WatchPropertiesUpdatedSignal) => void): Unsubscribe {
    return this.subscribeSignal(WATCH_IFACE, 'PropertiesUpdated', objectPath, null, params => {
      const [changed, removed] = params.deepUnpack() as [Record<string, string>, string[]];
      handler({ type: 'watch-properties-updated', changed, removed });
    });
  }

  closeWatch(objectPath: string): Promise<void> {
    return this.callOn(objectPath, WATCH_IFACE, 'Close', null, null, () => undefined);
  }

  private callRead<T>(method: string, params: GLib.Variant | null, resultType: string | null, unpack: (result: any) => T): Promise<T> {
    return this.callOn(ROOT_PATH, GRAPH_READ_IFACE, method, params, resultType, unpack);
  }

  private callWrite<T>(method: string, params: GLib.Variant | null, resultType: string | null, unpack: (result: any) => T): Promise<T> {
    return this.callOn(ROOT_PATH, GRAPH_WRITE_IFACE, method, params, resultType, unpack);
  }

  private callResolve<T>(method: string, params: GLib.Variant | null, resultType: string | null, unpack: (result: any) => T): Promise<T> {
    return this.callOn(ROOT_PATH, GRAPH_RESOLVE_IFACE, method, params, resultType, unpack);
  }

  private callOn<T>(objectPath: string, iface: string, method: string, params: GLib.Variant | null, resultType: string | null, unpack: (result: any) => T): Promise<T> {
    return new Promise((resolve, reject) => {
      const start = nowMs();
      Gio.DBus.session.call(
        BUS_NAME,
        objectPath,
        iface,
        method,
        params,
        resultType ? new GLib.VariantType(resultType) : null,
        Gio.DBusCallFlags.NONE,
        -1,
        null,
        (_conn: any, res: any) => {
          try {
            const result = Gio.DBus.session.call_finish(res);
            const value = unpack(result.deepUnpack());
            this.logLatency(`${iface}.${method}`, start);
            resolve(value);
          } catch (error) {
            this.logLatency(`${iface}.${method}`, start, true);
            reject(error);
          }
        },
      );
    });
  }

  private subscribeSignal(iface: string, signal: string, objectPath: string | null, arg0: string | null, handler: (params: GLib.Variant) => void): Unsubscribe {
    const id = Gio.DBus.session.signal_subscribe(
      BUS_NAME,
      iface,
      signal,
      objectPath,
      arg0,
      Gio.DBusSignalFlags.NONE,
      (_conn: any, _sender: any, _path: any, _iface: any, _signal: string, params: GLib.Variant) => handler(params),
    );
    return () => Gio.DBus.session.signal_unsubscribe(id);
  }

  private logLatency(method: string, start: number, failed = false): void {
    if (!this.traceLatency) return;
    const elapsed = (nowMs() - start).toFixed(1);
    const status = failed ? ' failed' : '';
    console.log(`[LocusDbus] ${method}${status} +${elapsed}ms`);
  }
}
