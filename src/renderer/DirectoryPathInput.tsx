import { useEffect, useId, useRef, useState, type ReactNode, type RefObject } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { request } from './api';
import { Icon } from './components';
import { useDirectoryCompletion } from './use-directory-completion';

export interface DirectoryPathInputProps {
  api: ManagerApi;
  value: string;
  onChange(value: string): void;
  onSubmit(): void;
  onError(message: string): void;
  inputRef?: RefObject<HTMLInputElement | null>;
  readOnly?: boolean;
  suggestionsEnabled?: boolean;
  onBrowse?: () => void;
  leading?: ReactNode;
  trailing?: ReactNode;
  hint?: ReactNode;
}

/** Shared folder entry, completion, keyboard handling, and host home indicator. */
export function DirectoryPathInput({ api, value, onChange, onSubmit, onError, inputRef, readOnly = false, suggestionsEnabled = true,
  onBrowse, leading, trailing, hint }: DirectoryPathInputProps) {
  const ownInput = useRef<HTMLInputElement>(null);
  const input = inputRef ?? ownInput;
  const browse = useRef<HTMLButtonElement>(null);
  const [home, setHome] = useState('~');
  const [focused, setFocused] = useState(false);
  const revision = useRef(0);
  const error = useRef(onError); error.current = onError;
  const listId = useId(), hintId = useId();
  const change = (path: string) => { revision.current++; onChange(path); };
  const completion = useDirectoryCompletion(api, value, suggestionsEnabled && !readOnly && focused,
    path => { change(path); input.current?.focus(); }, onError);
  useEffect(() => {
    let active = true;
    const ticket = revision.current;
    setHome('~');
    void request(() => api.getHomeDirectory()).then(result => {
      if (!active) return;
      if (result.ok) setHome(result.value.cwd);
      else if (revision.current === ticket) error.current(result.error.message);
    });
    return () => { active = false; revision.current++; };
  }, [api]);
  useEffect(() => {
    if (completion.active >= 0) document.getElementById(`${listId}-${completion.active}`)?.scrollIntoView?.({ block: 'nearest' });
  }, [completion.active, listId]);
  const submit = () => {
    if (readOnly || completion.composition.current || completion.enter()) return;
    revision.current++; completion.dismiss(); onSubmit();
  };
  return <form className="directory-path-picker" onSubmit={event => { event.preventDefault(); submit(); }}>
    <div className="directory-path">
      <div className="directory-line">
        {leading}
        <span className="directory-cwd" title={home} aria-label={`Working directory: ${home}`}>~</span><span className="directory-chevron" aria-hidden="true">›</span>
        <input ref={input} role="combobox" aria-autocomplete="list" aria-expanded={completion.matches.length > 0}
          aria-controls={completion.matches.length ? listId : undefined}
          aria-activedescendant={completion.matches.length && completion.active >= 0 ? `${listId}-${completion.active}` : undefined}
          aria-label="Folder path" aria-describedby={hint ? hintId : undefined} placeholder="folder path" value={value}
          readOnly={readOnly} autoComplete="off" spellCheck={false}
          onFocus={() => { setFocused(true); completion.reopen(); }} onClick={() => { setFocused(true); completion.reopen(); }}
          onBlur={() => { completion.dismiss(); setFocused(false); }}
          onCompositionStart={completion.startComposition} onCompositionEnd={completion.endComposition}
          onChange={event => { completion.edit(); change(event.target.value); }}
          onKeyDownCapture={event => {
            if (completion.composition.current || event.nativeEvent.isComposing || event.keyCode === 229) {
              if (event.key === 'Enter') event.preventDefault();
              event.stopPropagation(); return;
            }
            if (readOnly) return;
            if (event.key === 'Escape' && completion.dismiss()) { event.preventDefault(); event.stopPropagation(); }
            else if (event.key === 'Tab' && (!event.shiftKey || onBrowse)) {
              event.preventDefault(); event.stopPropagation(); revision.current++;
              if (event.shiftKey) browse.current?.focus(); else void completion.tab();
            } else if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && completion.move(event.key === 'ArrowDown' ? 1 : -1)) {
              event.preventDefault(); event.stopPropagation();
            } else if (event.key === 'Enter' && completion.enter()) { event.preventDefault(); event.stopPropagation(); }
          }} />
        {onBrowse && <button ref={browse} type="button" className="icon-button" aria-label="Browse folders" title="Browse folders" disabled={readOnly}
          onClick={() => { revision.current++; completion.dismiss(); onBrowse(); }}><Icon name="folder" /></button>}
        {trailing}
      </div>
      {completion.matches.length > 0 && <div id={listId} className="directory-completions" role="listbox" aria-label="Matching folders">
        {completion.matches.map((match, index) => <div key={match} id={`${listId}-${index}`} role="option" aria-selected={index === completion.active}
          className="directory-completion" title={match} onMouseDown={event => event.preventDefault()} onClick={() => completion.choose(index)}>
          <Icon name="folder" /><span>{match}</span>
        </div>)}
      </div>}
    </div>
    {hint && <div id={hintId} className="directory-hint">{hint}</div>}
  </form>;
}
