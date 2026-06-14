import { useCallback, useEffect, useState } from "react";
import { Field, Select, Toggle } from "../components/FormControls";
import type {
  FloatBarOrientation,
  FloatBarStyle,
  SettingsSnapshot,
  SettingsUpdate,
} from "../types/bridge";

interface Props {
  settings: SettingsSnapshot;
  saving: boolean;
  set: (patch: SettingsUpdate) => void;
}

function useDraftNumber(value: number) {
  const [draft, setDraft] = useState(value);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  const commit = useCallback(
    (next: number, onCommit: (value: number) => void) => {
      // Dedupe against the committed prop value, which is the persisted
      // source of truth. The parent's save is fire-and-forget, so we can't
      // observe success/failure here — comparing to `value` (rather than an
      // optimistically-advanced marker) means a failed save leaves the prop
      // unchanged and a re-commit of the same number still fires the retry.
      if (next === value) return;
      onCommit(next);
    },
    [value],
  );

  return { draft, setDraft, commit };
}

/**
 * Settings UI block for the floating capacity bar. Rendered as one row
 * in the Display tab — kept in this module so the Display tab only
 * imports a single component.
 */
export default function FloatBarSettingsSection({ settings, saving, set }: Props) {
  const opacity = useDraftNumber(settings.floatBarOpacity);
  const scale = useDraftNumber(settings.floatBarScale);
  const commitOpacity = () => {
    opacity.commit(opacity.draft, (value) => set({ floatBarOpacity: value }));
  };
  const commitScale = () => {
    scale.commit(scale.draft, (value) => set({ floatBarScale: value }));
  };

  return (
    <section className="settings-section">
      <h3 className="settings-section__title">Floating Bar</h3>
      <div className="settings-section__group">
        <Field
          label="Show Floating Bar"
          description="Always-on-top, transparent strip showing remaining capacity per provider."
          leading
        >
          <Toggle
            checked={settings.floatBarEnabled}
            disabled={saving}
            onChange={(v) => set({ floatBarEnabled: v })}
          />
        </Field>
        <Field
          label="Orientation"
          description="Horizontal sits above a taskbar; vertical sits on a screen edge."
        >
          <Select
            value={settings.floatBarOrientation}
            disabled={saving || !settings.floatBarEnabled}
            options={[
              { value: "horizontal", label: "Horizontal" },
              { value: "vertical", label: "Vertical" },
            ]}
            onChange={(v) => set({ floatBarOrientation: v as FloatBarOrientation })}
          />
        </Field>
        <Field
          label="Style"
          description="Choose the original floating glass look or the Windows taskbar widget look."
        >
          <Select
            value={settings.floatBarStyle}
            disabled={saving || !settings.floatBarEnabled}
            options={[
              { value: "floating", label: "Floating glass" },
              { value: "taskbar", label: "Taskbar widget" },
            ]}
            onChange={(v) => set({ floatBarStyle: v as FloatBarStyle })}
          />
        </Field>
        <Field
          label={`Opacity (${opacity.draft}%)`}
          description="Lower values make the bar more see-through."
        >
          <input
            type="range"
            min={30}
            max={100}
            step={5}
            value={opacity.draft}
            disabled={!settings.floatBarEnabled}
            onChange={(e) => opacity.setDraft(Number(e.target.value))}
            onPointerUp={commitOpacity}
            onTouchEnd={commitOpacity}
            onBlur={commitOpacity}
            onKeyUp={commitOpacity}
            aria-label="Floating bar opacity"
          />
        </Field>
        <Field
          label={`Size (${scale.draft}%)`}
          description="Scales the floating bar icons, text, and pill spacing."
        >
          <input
            type="range"
            min={75}
            max={200}
            step={5}
            value={scale.draft}
            disabled={!settings.floatBarEnabled}
            onChange={(e) => scale.setDraft(Number(e.target.value))}
            onPointerUp={commitScale}
            onTouchEnd={commitScale}
            onBlur={commitScale}
            onKeyUp={commitScale}
            aria-label="Floating bar size"
          />
        </Field>
        <Field
          label="Show Reset Time Inline"
          description="Shows the reset time beside each provider percentage with a reset icon."
          leading
        >
          <Toggle
            checked={settings.floatBarShowResetInline}
            disabled={saving || !settings.floatBarEnabled}
            onChange={(v) => set({ floatBarShowResetInline: v })}
          />
        </Field>
        <Field
          label="Invert Colors"
          description="Switches to dark text on light glass for bright backgrounds."
          leading
        >
          <Toggle
            checked={settings.floatBarDarkText}
            disabled={saving || !settings.floatBarEnabled}
            onChange={(v) => set({ floatBarDarkText: v })}
          />
        </Field>
        <Field
          label="Click-Through"
          description="Mouse clicks pass through to the window underneath — pure overlay mode."
          leading
        >
          <Toggle
            checked={settings.floatBarClickThrough}
            disabled={saving || !settings.floatBarEnabled}
            onChange={(v) => set({ floatBarClickThrough: v })}
          />
        </Field>
      </div>
    </section>
  );
}
