import { useRef, useState } from 'react';
import './file-layout-resizer.css';

interface LayoutResizerProps {
  label: string;
  orientation: 'vertical' | 'horizontal';
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  onChange: (value: number) => void;
  /** Right-edge panels grow when the handle moves left. */
  reverse?: boolean;
  className?: string;
  hidden?: boolean;
  onDraggingChange?: (dragging: boolean) => void;
}

export function LayoutResizer({ label, orientation, value, min, max, defaultValue, onChange, reverse, className = '', hidden, onDraggingChange }: LayoutResizerProps) {
  const drag = useRef<{ id: number; coordinate: number; value: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const vertical = orientation === 'vertical';
  const direction = reverse ? -1 : 1;
  const change = (next: number) => onChange(Math.max(min, Math.min(max, next)));
  const stop = () => { drag.current = null; setDragging(false); onDraggingChange?.(false); };
  return <>
    <div className={`layout-resizer layout-resizer-${orientation}${dragging ? ' layout-resizer-dragging' : ''} ${className}`}
      role="separator" aria-label={label} aria-orientation={orientation} aria-valuemin={Math.round(min)} aria-valuemax={Math.round(max)} aria-valuenow={Math.round(value)}
      tabIndex={0} hidden={hidden} onDoubleClick={() => change(defaultValue)}
      onKeyDown={event => {
        const decrease = vertical ? 'ArrowLeft' : 'ArrowUp', increase = vertical ? 'ArrowRight' : 'ArrowDown';
        if (event.key === decrease || event.key === increase) {
          event.preventDefault(); change(value + (event.key === decrease ? -1 : 1) * direction * (event.shiftKey ? 32 : 4));
        } else if (event.key === 'Home') { event.preventDefault(); change(defaultValue); }
      }}
      onPointerDown={event => {
        if (event.button !== 0 || drag.current) return;
        event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { id: event.pointerId, coordinate: vertical ? event.clientX : event.clientY, value };
        setDragging(true); onDraggingChange?.(true);
      }}
      onPointerMove={event => {
        const start = drag.current;
        if (start?.id === event.pointerId) change(start.value + ((vertical ? event.clientX : event.clientY) - start.coordinate) * direction);
      }}
      onPointerUp={event => {
        if (drag.current?.id !== event.pointerId) return;
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        stop();
      }} onPointerCancel={stop} onLostPointerCapture={stop} />
    {dragging && !hidden && <div className={`layout-resize-shield layout-resize-shield-${orientation}`} aria-hidden="true" />}
  </>;
}
