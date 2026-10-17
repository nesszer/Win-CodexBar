import { type KeyboardEvent, type MouseEvent, useLayoutEffect, useRef, useState } from "react";
import {
  type TrendModel,
  type TrendTick,
  domainDayCount,
  needsScrolling,
  slotFraction,
  visibleDayCount,
} from "../../lib/spendTrendModel";
import { barRect, fractionAt, niceTicks, segmentPath, slotRect, valueY } from "./stackedTrendGeometry";

/**
 * Stacked spend bars over day, week, month or hour slots (upstream 0.73.0
 * `SpendTrendChart`). Hand-built SVG: hover inspects a slot, a click or
 * Enter activates it (the caller drills down), arrow keys walk the buckets.
 * A daily chart longer than 32 days scrolls and opens on its newest end.
 */
export interface StackedTrendChartProps {
  model: TrendModel;
  ariaLabel: string;
  ariaDescription: string;
  ticks: TrendTick[];
  colorFor: (sourceId: string) => string;
  sourceName: (sourceId: string) => string;
  bucketLabel: (key: number) => string;
  formatCost: (value: number) => string;
  formatAxisCost: (value: number) => string;
  /** Slot under the pointer, else the pinned one; draws the rule mark. */
  activeKey: number | null;
  pinnedKey: number | null;
  onHover: (key: number | null) => void;
  onPin: (key: number) => void;
  onActivate: (key: number) => void;
  height?: number;
}

const DEFAULT_WIDTH = 560;
const Y_AXIS_WIDTH = 52;

function useMeasuredWidth() {
  const ref = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    const measure = () => {
      const next = node.clientWidth;
      if (next > 0) setWidth(next);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

export function StackedTrendChart({
  model,
  ariaLabel,
  ariaDescription,
  ticks,
  colorFor,
  sourceName,
  bucketLabel,
  formatCost,
  formatAxisCost,
  activeKey,
  pinnedKey,
  onHover,
  onPin,
  onActivate,
  height = 190,
}: StackedTrendChartProps) {
  const [hostRef, hostWidth] = useMeasuredWidth();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const optionRefs = useRef(new Map<number, SVGGElement>());
  const viewport = Math.max(120, hostWidth - Y_AXIS_WIDTH);
  const scrolls = needsScrolling(model);
  const width = scrolls ? (viewport * domainDayCount(model)) / visibleDayCount(model) : viewport;
  const frame = { width, height };
  const yTicks = niceTicks(model.yMax);

  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (node && scrolls) node.scrollLeft = node.scrollWidth;
  }, [scrolls, width, model.domain.start, model.domain.end]);

  const geometry = new Map(
    model.slots.map((slot) => [slot.key, slotRect(frame, slotFraction(model, slot))] as const),
  );
  const keyAt = (x: number): number | null => {
    const fraction = fractionAt(frame, x);
    if (fraction == null) return null;
    const slot = model.slots.find((entry) => {
      const position = slotFraction(model, entry);
      return fraction >= position.start && fraction < position.start + position.width;
    });
    return slot?.key ?? null;
  };
  const pointerKey = (event: MouseEvent<SVGRectElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return keyAt(event.clientX - bounds.left);
  };

  const keys = model.buckets.map((bucket) => bucket.key);
  const focusKey = pinnedKey != null && keys.includes(pinnedKey) ? pinnedKey : (model.peak?.key ?? keys[keys.length - 1]);
  const onKeyDown = (event: KeyboardEvent<SVGSVGElement>) => {
    const index = Math.max(0, keys.indexOf(focusKey));
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = Math.max(0, index - 1);
    else if (event.key === "ArrowRight") next = Math.min(keys.length - 1, index + 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = keys.length - 1;
    else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onActivate(focusKey);
      return;
    }
    if (next == null) return;
    event.preventDefault();
    onPin(keys[next]);
    optionRefs.current.get(keys[next])?.focus();
  };

  const rule = activeKey != null ? geometry.get(activeKey) : undefined;

  return (
    <div className="spend-trend-chart" ref={hostRef}>
      <div className="spend-trend-chart__y-axis" style={{ width: Y_AXIS_WIDTH, height }} aria-hidden="true">
        {yTicks.map((value) => (
          <span key={value} style={{ top: valueY(frame, value, model.yMax) }}>
            {formatAxisCost(value)}
          </span>
        ))}
      </div>
      <div
        className={`spend-trend-chart__scroller${scrolls ? " is-scrolling" : ""}`}
        ref={scrollRef}
        data-testid="spend-trend-scroller"
        data-scrolls={scrolls ? "true" : "false"}
      >
        <svg
          width={width}
          height={height}
          viewBox={`0 0 ${width} ${height}`}
          className="spend-trend-chart__svg"
          role="listbox"
          aria-orientation="horizontal"
          aria-label={ariaLabel}
          aria-description={ariaDescription}
          data-testid="spend-trend-chart"
          onKeyDown={onKeyDown}
        >
          {yTicks.map((value) => (
            <line
              key={value}
              className="spend-trend-chart__grid"
              x1={0}
              x2={width}
              y1={valueY(frame, value, model.yMax)}
              y2={valueY(frame, value, model.yMax)}
            />
          ))}
          {model.buckets.map((bucket) => {
            const slot = geometry.get(bucket.key);
            if (!slot) return null;
            const bar = barRect(slot);
            const top = bucket.segments[bucket.segments.length - 1];
            const label = [
              `${bucketLabel(bucket.key)}: ${formatCost(bucket.total)}`,
              ...bucket.segments.map((segment) => `${sourceName(segment.sourceId)} ${formatCost(segment.cost)}`),
            ].join(", ");
            return (
              <g
                key={bucket.key}
                ref={(node) => {
                  if (node) optionRefs.current.set(bucket.key, node);
                  else optionRefs.current.delete(bucket.key);
                }}
                role="option"
                aria-selected={bucket.key === pinnedKey}
                aria-label={label}
                tabIndex={bucket.key === focusKey ? 0 : -1}
                className="spend-trend-chart__bucket"
                data-key={bucket.key}
                onFocus={() => onPin(bucket.key)}
              >
                {bucket.segments.map((segment) => (
                  <path
                    key={segment.sourceId}
                    d={segmentPath(
                      bar.x,
                      valueY(frame, segment.end, model.yMax),
                      bar.width,
                      valueY(frame, segment.start, model.yMax),
                      segment === top,
                    )}
                    fill={colorFor(segment.sourceId)}
                    data-source={segment.sourceId}
                    data-rounded={segment === top ? "true" : "false"}
                  />
                ))}
              </g>
            );
          })}
          {rule && (
            <line
              className="spend-trend-chart__rule"
              x1={rule.x + rule.width / 2}
              x2={rule.x + rule.width / 2}
              y1={0}
              y2={height}
              data-testid="spend-trend-rule"
            />
          )}
          <rect
            className="spend-trend-chart__hit"
            x={0}
            y={0}
            width={width}
            height={height}
            fill="transparent"
            aria-hidden="true"
            data-testid="spend-trend-hit"
            onMouseMove={(event) => onHover(pointerKey(event))}
            onMouseLeave={() => onHover(null)}
            onClick={(event) => {
              const key = pointerKey(event);
              if (key != null) onActivate(key);
            }}
          />
        </svg>
        <div className="spend-trend-chart__x-axis" style={{ width }} aria-hidden="true">
          {ticks.map((tick) => (
            <span
              key={`${tick.position}:${tick.label}`}
              className={`spend-trend-chart__tick spend-trend-chart__tick--${tick.anchor}`}
              style={{ left: slotRect(frame, { start: tick.position, width: 0 }).x }}
            >
              {tick.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
