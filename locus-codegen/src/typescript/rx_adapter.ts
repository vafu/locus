
const SHARE_REPLAY_ONE = { bufferSize: 1, refCount: true } as const;

function present(value: OptionalNodeId): string {
  return value ?? '';
}

function cacheKey(parts: readonly unknown[]): string {
  return parts
    .map(part => Array.isArray(part) ? part.join('\u0001') : String(part))
    .join('\u0000');
}

function sameArray(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export class LocusObservableAdapterBase {
  private readonly watchCache = new Map<string, Observable<LocusWatch>>();
  private readonly pathCache = new Map<string, Observable<OptionalNodeId>>();
  private readonly pathAllCache = new Map<string, Observable<NodeId[]>>();
  private readonly resolveCache = new Map<string, Observable<string>>();
  private readonly resolvedPropertyCache = new Map<string, Observable<string>>();
  private readonly pathPropertyCache = new Map<string, Observable<string>>();
  private readonly propertyCache = new Map<string, Observable<string>>();
  private readonly propertiesCache = new Map<string, Observable<Record<string, string>>>();
  private readonly pathPropertiesCache = new Map<string, Observable<Record<string, string>>>();
  private readonly findSubjectsCache = new Map<string, Observable<NodeId[]>>();
  private readonly sourcesCache = new Map<string, Observable<NodeId[]>>();
  private readonly targetsCache = new Map<string, Observable<NodeId[]>>();

  constructor(protected readonly client: LocusDbusClient = new LocusDbusClient()) {}

  watch$(source: NodeId, relations: readonly string[]): Observable<LocusWatch> {
    const key = cacheKey(['watch', source, relations]);
    let cached = this.watchCache.get(key);
    if (!cached) {
      cached = new Observable<LocusWatch>(subscriber => {
        let watch: LocusWatch | null = null;
        let closed = false;
        this.client.watchNode(source, [...relations] as Relation[])
          .then(value => {
            if (closed) {
              value.close().catch(error => console.error('[LocusRx] close stale watch failed:', error));
              return;
            }
            watch = value;
            subscriber.next(value);
          })
          .catch(error => subscriber.error(error));
        return () => {
          closed = true;
          if (watch) {
            watch.close().catch(error => console.error('[LocusRx] close watch failed:', error));
          }
        };
      }).pipe(shareReplay(SHARE_REPLAY_ONE));
      this.watchCache.set(key, cached);
    }
    return cached;
  }

  resolve$(source: NodeId, relations: readonly string[]): Observable<string> {
    const key = cacheKey(['resolve', source, relations]);
    let cached = this.resolveCache.get(key);
    if (!cached) {
      cached = this.watch$(source, relations).pipe(
        switchMap(watch => new Observable<string>(subscriber => {
          watch.target().then(value => subscriber.next(present(value))).catch(error => subscriber.error(error));
          const unsubscribe = watch.onTargetChanged(value => subscriber.next(present(value)));
          return unsubscribe;
        })),
        distinctUntilChanged(),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.resolveCache.set(key, cached);
    }
    return cached;
  }

  path$(name: NamedPath, source?: NodeId): Observable<OptionalNodeId> {
    const spec = path(name);
    const resolvedSource = source ?? spec.from;
    const key = cacheKey(['path', name, resolvedSource]);
    let cached = this.pathCache.get(key);
    if (!cached) {
      cached = this.watch$(resolvedSource, spec.path).pipe(
        switchMap(watch => new Observable<OptionalNodeId>(subscriber => {
          watch.target().then(value => subscriber.next(value)).catch(error => subscriber.error(error));
          const unsubscribe = watch.onTargetChanged(value => subscriber.next(value));
          return unsubscribe;
        })),
        distinctUntilChanged(),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.pathCache.set(key, cached);
    }
    return cached;
  }

  pathAll$(name: NamedPath, source?: NodeId): Observable<NodeId[]> {
    const spec = path(name);
    const resolvedSource = source ?? spec.from;
    const key = cacheKey(['path-all', name, resolvedSource]);
    let cached = this.pathAllCache.get(key);
    if (!cached) {
      cached = new Observable<NodeId[]>(subscriber => {
        const relations = new Set<string>(spec.path);
        const refresh = () => {
          this.client.resolveAllPath(name, resolvedSource)
            .then(targets => subscriber.next(targets))
            .catch(error => subscriber.error(error));
        };

        refresh();
        const unsubscribeAdded = this.client.onLinkAdded(signal => {
          if (relations.has(signal.relation)) refresh();
        });
        const unsubscribeRemoved = this.client.onLinkRemoved(signal => {
          if (relations.has(signal.relation)) refresh();
        });
        const unsubscribeSet = this.client.onLinkSet(signal => {
          if (relations.has(signal.relation)) refresh();
        });

        return () => {
          unsubscribeAdded();
          unsubscribeRemoved();
          unsubscribeSet();
        };
      }).pipe(
        distinctUntilChanged(sameArray),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.pathAllCache.set(key, cached);
    }
    return cached;
  }

  pathString$(name: NamedPath, source?: NodeId): Observable<string> {
    return this.path$(name, source).pipe(
      map(present),
      distinctUntilChanged(),
      shareReplay(SHARE_REPLAY_ONE),
    );
  }

  pathProperty$(name: NamedPath, key: string, source?: NodeId): Observable<string> {
    const spec = path(name);
    const resolvedSource = source ?? spec.from;
    const cache = cacheKey(['path-property', name, resolvedSource, key]);
    let cached = this.pathPropertyCache.get(cache);
    if (!cached) {
      cached = this.watch$(resolvedSource, spec.path).pipe(
        switchMap(watch => new Observable<string>(subscriber => {
          watch.property(key).then(value => subscriber.next(value)).catch(error => subscriber.error(error));
          const unsubscribe = watch.onPropertyUpdated(key, value => subscriber.next(value));
          return unsubscribe;
        })),
        distinctUntilChanged(),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.pathPropertyCache.set(cache, cached);
    }
    return cached;
  }

  resolvedProperty$(source: NodeId, relations: readonly string[], key: string): Observable<string> {
    const cache = cacheKey(['resolved-property', source, relations, key]);
    let cached = this.resolvedPropertyCache.get(cache);
    if (!cached) {
      cached = this.watch$(source, relations).pipe(
        switchMap(watch => new Observable<string>(subscriber => {
          watch.property(key).then(value => subscriber.next(value)).catch(error => subscriber.error(error));
          const unsubscribe = watch.onPropertyUpdated(key, value => subscriber.next(value));
          return unsubscribe;
        })),
        distinctUntilChanged(),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.resolvedPropertyCache.set(cache, cached);
    }
    return cached;
  }

  property$(subject: NodeId, key: string): Observable<string> {
    const cache = cacheKey(['property', subject, key]);
    let cached = this.propertyCache.get(cache);
    if (!cached) {
      cached = new Observable<string>(subscriber => {
        this.client.property(subject, key as PropertyKey)
          .then(value => subscriber.next(present(value)))
          .catch(error => subscriber.error(error));
        const changed = this.client.onPropertyChanged(signal => {
          if (signal.subject === subject && signal.key === key) subscriber.next(signal.value);
        });
        const removed = this.client.onPropertyRemoved(signal => {
          if (signal.subject === subject && signal.key === key) subscriber.next('');
        });
        return () => {
          changed();
          removed();
        };
      }).pipe(
        distinctUntilChanged(),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.propertyCache.set(cache, cached);
    }
    return cached;
  }

  properties$(subject: NodeId): Observable<Record<string, string>> {
    let cached = this.propertiesCache.get(subject);
    if (!cached) {
      cached = new Observable<Record<string, string>>(subscriber => {
        const refresh = () => {
          this.client.properties(subject)
            .then(properties => subscriber.next(properties))
            .catch(error => subscriber.error(error));
        };

        refresh();
        const changed = this.client.onPropertyChanged(signal => {
          if (signal.subject === subject) refresh();
        });
        const removed = this.client.onPropertyRemoved(signal => {
          if (signal.subject === subject) refresh();
        });
        return () => {
          changed();
          removed();
        };
      }).pipe(shareReplay(SHARE_REPLAY_ONE));
      this.propertiesCache.set(subject, cached);
    }
    return cached;
  }

  findSubjects$(key: string, value: string): Observable<NodeId[]> {
    const cache = cacheKey(['find-subjects', key, value]);
    let cached = this.findSubjectsCache.get(cache);
    if (!cached) {
      cached = new Observable<NodeId[]>(subscriber => {
        const refresh = () => {
          this.client.findSubjects(key as PropertyKey, value)
            .then(subjects => subscriber.next(subjects))
            .catch(error => subscriber.error(error));
        };

        refresh();
        const changed = this.client.onPropertyChanged(signal => {
          if (signal.key === key) refresh();
        });
        const removed = this.client.onPropertyRemoved(signal => {
          if (signal.key === key) refresh();
        });
        return () => {
          changed();
          removed();
        };
      }).pipe(
        distinctUntilChanged(sameArray),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.findSubjectsCache.set(cache, cached);
    }
    return cached;
  }

  pathProperties$(name: NamedPath, source?: NodeId): Observable<Record<string, string>> {
    const spec = path(name);
    const resolvedSource = source ?? spec.from;
    const cache = cacheKey(['path-properties', name, resolvedSource]);
    let cached = this.pathPropertiesCache.get(cache);
    if (!cached) {
      cached = this.watch$(resolvedSource, spec.path).pipe(
        switchMap(watch => new Observable<Record<string, string>>(subscriber => {
          watch.properties().then(value => subscriber.next(value)).catch(error => subscriber.error(error));
          const unsubscribe = watch.onPropertiesUpdated(() => {
            watch.properties().then(value => subscriber.next(value)).catch(error => subscriber.error(error));
          });
          return unsubscribe;
        })),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.pathPropertiesCache.set(cache, cached);
    }
    return cached;
  }

  numberProperty$(subject: NodeId, key: string, fallback = 0): Observable<number> {
    return this.property$(subject, key).pipe(
      map(value => {
        const number = Number(value);
        return Number.isFinite(number) ? number : fallback;
      }),
      distinctUntilChanged(),
      shareReplay(SHARE_REPLAY_ONE),
    );
  }

  booleanProperty$(subject: NodeId, key: string): Observable<boolean> {
    return this.property$(subject, key).pipe(
      map(value => value === 'true'),
      distinctUntilChanged(),
      shareReplay(SHARE_REPLAY_ONE),
    );
  }

  sources$(target: NodeId, relation: string): Observable<NodeId[]> {
    const key = cacheKey(['sources', target, relation]);
    let cached = this.sourcesCache.get(key);
    if (!cached) {
      cached = new Observable<NodeId[]>(subscriber => {
        const refresh = () => {
          this.client.sources(target, relation as Relation)
            .then(sources => subscriber.next(sources))
            .catch(error => subscriber.error(error));
        };

        refresh();
        const unsubscribeAdded = this.client.onLinkAdded(signal => {
          if (signal.relation === relation && signal.target === target) refresh();
        });
        const unsubscribeRemoved = this.client.onLinkRemoved(signal => {
          if (signal.relation === relation && signal.target === target) refresh();
        });
        const unsubscribeSet = this.client.onLinkSet(signal => {
          if (signal.relation === relation && (signal.target === target || signal.oldTargets.includes(target))) refresh();
        });

        return () => {
          unsubscribeAdded();
          unsubscribeRemoved();
          unsubscribeSet();
        };
      }).pipe(
        distinctUntilChanged(sameArray),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.sourcesCache.set(key, cached);
    }
    return cached;
  }

  targets$(source: NodeId, relation: string): Observable<NodeId[]> {
    const key = cacheKey(['targets', source, relation]);
    let cached = this.targetsCache.get(key);
    if (!cached) {
      cached = new Observable<NodeId[]>(subscriber => {
        const refresh = () => {
          this.client.targets(source, relation as Relation)
            .then(targets => subscriber.next(targets))
            .catch(error => subscriber.error(error));
        };

        refresh();
        const unsubscribeAdded = this.client.onLinkAdded(signal => {
          if (signal.relation === relation && signal.source === source) refresh();
        });
        const unsubscribeRemoved = this.client.onLinkRemoved(signal => {
          if (signal.relation === relation && signal.source === source) refresh();
        });
        const unsubscribeSet = this.client.onLinkSet(signal => {
          if (signal.relation === relation && signal.source === source) refresh();
        });

        return () => {
          unsubscribeAdded();
          unsubscribeRemoved();
          unsubscribeSet();
        };
      }).pipe(
        distinctUntilChanged(sameArray),
        shareReplay(SHARE_REPLAY_ONE),
      );
      this.targetsCache.set(key, cached);
    }
    return cached;
  }
}
