import {useCallback, useEffect, useRef, useState, type RefObject} from 'react';
import {Keyboard} from 'react-native';
import type {NativeLayoutPresentation, NativeViewDriver, NativeViewSession} from '../rust-client/native-terminal';
import {sameUploadIdentity, type UploadIdentity} from './upload';
import {retirementRecord,type KeyboardClearReason,type KeyboardRetirement,type LayoutComparisonOutcome} from './keyboard-retirement';

type Mode = 'closed' | 'system' | 'special';
interface Context {
  view: NativeViewSession | null;
  current: RefObject<NativeViewSession | null>;
  driver: NativeViewDriver;
  tabId: string;
  connectionEpoch: string | number;
  readOnly: boolean;
}
interface Request {
  mode: Exclude<Mode, 'closed'>;
  phase: 'ready' | 'recovering' | 'layout';
  view: NativeViewSession;
  driver: NativeViewDriver;
  tabId: string;
  connectionEpoch: string | number;
  identity: UploadIdentity;
}
function sameScope(request: Request, context: Context): boolean {
  return !context.readOnly && !request.view.readOnly && request.view === context.view
    && request.view === context.current.current && request.driver === context.driver
    && request.tabId === context.tabId && request.connectionEpoch === context.connectionEpoch;
}
// Observation lifetime only. Do not query native readiness or ownership here.
function sameActionScope(scope:Context,now:Context):boolean {
  return scope.view===now.view && scope.view===now.current.current && scope.driver===now.driver
    && scope.tabId===now.tabId && scope.connectionEpoch===now.connectionEpoch && scope.readOnly===now.readOnly;
}
function installed(request: Request, context: Context): boolean {
  return sameScope(request, context) && request.view.getState() === 'attached';
}
function valid(request: Request, context: Context): boolean {
  if (!sameScope(request, context)) return false;
  if (request.phase === 'layout' && request.view.getState() === 'reconnecting') return true;
  if (!installed(request, context)) return false;
  const identity = request.view.getIdentity();
  return (request.phase === 'recovering' && identity === null) || sameUploadIdentity(identity, request.identity);
}

/** Desired UI mode belongs to one original owned view, never to IME visibility. */
export function useOwnedKeyboardMode(context: Context, keyboardVisible: boolean) {
  const latest = useRef(context); latest.current = context;
  const request = useRef<Request | null>(null);
  const action = useRef<{intent:'show'|'hide';scope:Context;revision:object;first:KeyboardRetirement|null}|null>(null);
  const [presentation, setPresentation] = useState<{mode:Mode;revision:object;retirement:KeyboardRetirement|null}>(() => ({mode:'closed',revision:{},retirement:null}));
  const revision = useRef(presentation.revision);
  const firstRetirement = useCallback((reason:KeyboardClearReason,comparison?:LayoutComparisonOutcome,source?:NativeViewSession) => {
    const accepted=action.current,now=latest.current;
    if(!accepted)return null;
    if(!sameActionScope(accepted.scope,now)){action.current=null;return null;}
    if(source && accepted.scope.view!==source)return accepted.first;
    if(!accepted.first && accepted.revision===revision.current)accepted.first=retirementRecord(accepted.intent,reason,comparison);
    return accepted.first;
  }, []);
  const clear = useCallback((dismiss = true,reason:KeyboardClearReason='unknown',comparison?:LayoutComparisonOutcome,source?:NativeViewSession) => {
    const retirement=firstRetirement(reason,comparison,source);
    // Retirement invalidates queued callbacks even when a new full control has
    // the same UI identity triple. Publish it before React can render again.
    const nextRevision = {}; revision.current = nextRevision;
    request.current = null; setPresentation({mode:'closed',revision:nextRevision,retirement});
    if (dismiss) Keyboard.dismiss();
  }, [firstRetirement]);
  const renderedView = context.view;
  const renderedIdentity = renderedView?.getIdentity();
  const actionIdentity = renderedIdentity ? {...renderedIdentity} : null;
  const actionRevision = presentation.revision;
  const choose = (next: Mode,intent?:'show'|'hide'):boolean => {
    if (actionRevision !== revision.current) return false;
    const now = latest.current;
    // Neither SHOW nor HIDE from an old rendering can act on a new context.
    if (!renderedView || renderedView !== now.view || renderedView !== now.current.current
      || context.driver !== now.driver || context.tabId !== now.tabId || context.connectionEpoch !== now.connectionEpoch) return false;
    if (next === 'closed') {
      if (request.current && (!actionIdentity || !sameUploadIdentity(actionIdentity, request.current.identity))) return false;
      if(intent)action.current={intent,scope:context,revision:actionRevision,first:null};
      clear(true,'explicit_mode_close'); return true;
    }
    if (!actionIdentity || context.readOnly) return false;
    const original: Request = {mode: next, phase: 'ready', view: renderedView, driver: context.driver,
      tabId: context.tabId, connectionEpoch: context.connectionEpoch, identity: actionIdentity};
    if (!valid(original, now)) return false;
    if(intent)action.current={intent,scope:context,revision:actionRevision,first:null};
    const retirement=intent?null:request.current?.mode!==next?firstRetirement('explicit_mode_close'):action.current?.first??null;
    request.current = original; setPresentation({mode:next,revision:actionRevision,retirement});return true;
  };
  const recovering = useCallback((view: NativeViewSession): boolean => {
    const original = request.current;
    if (!original || original.view !== view || !valid(original, latest.current)) return false;
    if (original.phase !== 'layout') request.current = {...original, phase: 'recovering'};
    return true;
  }, []);
  const ready = useCallback((view: NativeViewSession) => {
    const original = request.current;
    if (!original) return;
    if (original.phase === 'layout' && original.view === view && sameScope(original, latest.current)
      && view.getState() === 'reconnecting') return;
    if (original.view !== view || !installed(original, latest.current)
      || !sameUploadIdentity(view.getIdentity(), original.identity)) { clear(true,'ready_context_lost',undefined,view); return; }
    request.current = {...original, phase: 'ready'};
  }, [clear]);

  const layoutPending = useCallback((view: NativeViewSession, notice: NativeLayoutPresentation): boolean => {
    const original = request.current;
    if (!original || original.view !== view || notice.phase !== 'pending' || !sameScope(original, latest.current)
      || view.getState() !== 'reconnecting' || !sameUploadIdentity(notice.identity, original.identity)) return false;
    request.current = {...original, phase: 'layout'}; return true;
  }, []);
  const layoutFinished = useCallback((view: NativeViewSession, notice?: NativeLayoutPresentation) => {
    const original = request.current;
    if (original?.phase !== 'layout') return;
    const identity = view.getIdentity();
    if (original.view !== view || notice?.phase !== 'validated' || !installed(original, latest.current)
      || !sameUploadIdentity(notice.identity, original.identity) || identity && !sameUploadIdentity(identity, original.identity)) { clear(true,'layout_validation_lost',notice?.kind==='layoutRetired'?notice.outcome:undefined,view); return; }
    request.current = {...original, phase: identity ? 'ready' : 'recovering'};
  }, [clear]);

  useEffect(() => {
    // A show snapshot cannot create or revive a desired mode. A hide during a
    // validated recovery may be the native readiness fence, not user HIDE.
    const original = request.current;
    if (keyboardVisible || original?.mode !== 'system') return;
    if (original.phase !== 'ready' && valid(original, latest.current)) return;
    // After ready, a late fence hide is indistinguishable from ordinary OS hide.
    clear(false,'visibility_hidden_after_ready');
  }, [keyboardVisible, clear]);
  useEffect(() => {
    if (request.current && !valid(request.current, latest.current)) clear(true,'request_context_invalid');
  });
  const original = request.current;
  const actionCurrent=action.current && sameActionScope(action.current.scope,context);
  return {mode: original && valid(original, context) ? presentation.mode : 'closed', retirement:actionCurrent?presentation.retirement:null,
    actionIntent:actionCurrent?action.current?.intent??null:null,
    choose, clear, recovering, ready, layoutPending, layoutFinished};
}
