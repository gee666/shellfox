import { useEffect } from 'react';
import type { RefObject } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { visibilityReporter } from './visibility-reporter';

const ROW = '[role="listitem"][data-session-id]';
const TERMINAL = '[data-terminal-session-id]';
const TARGETS = `${ROW}, ${TERMINAL}`;

/** IntersectionObserver clips against both the viewport and scroll ancestors.
 * Mounted overscan and retained keyboard/menu rows are not polling targets. */
export function useVisibleSessions(api: ManagerApi, rootRef: RefObject<HTMLElement | null>, machineId?: string) {
  useEffect(() => {
    const root = rootRef.current;
    if (!root || typeof api.setVisibleSessions !== 'function') return;
    const targets = new Set<HTMLElement>();
    const intersecting = new Set<HTMLElement>();
    const report = visibilityReporter(api);
    let frame: number | undefined;
    let active = true;

    function publish() {
      frame = undefined;
      const ids = document.hidden ? [] : [...intersecting]
        .filter(node => root!.contains(node) && !node.closest('[hidden]'))
        .map(node => node.dataset.sessionId ?? node.dataset.terminalSessionId)
        .filter((id): id is string => !!id);
      report([...new Set(ids)].sort());
    }
    function schedule() {
      if (document.hidden || frame !== undefined) return;
      frame = requestAnimationFrame(publish);
    }
    const observer = typeof IntersectionObserver === 'undefined' ? null : new IntersectionObserver(entries => {
      if (!active || document.hidden) return;
      for (const entry of entries) {
        const node = entry.target as HTMLElement;
        if (!targets.has(node)) continue;
        // Edge-touching rectangles can be isIntersecting with zero area.
        if (entry.isIntersecting && entry.intersectionRatio > 0) intersecting.add(node);
        else intersecting.delete(node);
      }
      schedule();
    }, { root: null, rootMargin: '0px', threshold: [0, Number.EPSILON] });

    function syncTargets() {
      const next = new Set([...root!.querySelectorAll<HTMLElement>(TARGETS)].filter(node => {
        const host = node.closest<HTMLElement>('[data-machine-id]')?.dataset.machineId;
        return machineId === undefined || host === machineId;
      }));
      for (const node of targets) {
        if (next.has(node)) continue;
        observer?.unobserve(node);
        targets.delete(node);
        intersecting.delete(node);
      }
      for (const node of next) {
        if (targets.has(node)) continue;
        targets.add(node);
        observer?.observe(node);
      }
      schedule();
    }
    function containsTarget(node: Node) {
      return node instanceof Element && (node.matches(TARGETS) || !!node.querySelector(TARGETS));
    }
    const mutations = new MutationObserver(records => {
      if (document.hidden) return;
      // Ignore status text and xterm output. Only row/terminal membership or
      // visibility attributes change the observed elements.
      if (records.some(record => record.type === 'attributes' || [...record.addedNodes, ...record.removedNodes].some(containsTarget))) syncTargets();
    });
    mutations.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-session-id', 'data-terminal-session-id', 'data-machine-id', 'hidden'] });
    function visibilityChanged() {
      if (frame !== undefined) { cancelAnimationFrame(frame); frame = undefined; }
      observer?.disconnect();
      observer?.takeRecords();
      targets.clear();
      intersecting.clear();
      if (document.hidden) report([]);
      else syncTargets(); // Ask for fresh geometry, including scrolling while hidden.
    }
    document.addEventListener('visibilitychange', visibilityChanged);
    report([]); // No known visible targets until the first intersection delivery.
    if (!document.hidden) syncTargets();
    return () => {
      active = false;
      mutations.disconnect();
      observer?.disconnect();
      document.removeEventListener('visibilitychange', visibilityChanged);
      if (frame !== undefined) cancelAnimationFrame(frame);
      report([]);
    };
  }, [api, rootRef, machineId]);
}
