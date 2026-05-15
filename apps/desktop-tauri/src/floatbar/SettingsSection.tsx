import { Field, Select, Toggle } from "../components/FormControls";
import type {
  FloatBarOrientation,
  SettingsSnapshot,
  SettingsUpdate,
} from "../types/bridge";

interface Props {
  settings: SettingsSnapshot;
  saving: boolean;
  set: (patch: SettingsUpdate) => void;
}

/**
 * Settings UI block for the floating capacity bar. Rendered as one row
 * in the Display tab — kept in this module so the Display tab only
 * imports a single component.
 */
export default function FloatBarSettingsSection({ settings, saving, set }: Props) {
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
          label={`Opacity (${settings.floatBarOpacity}%)`}
          description="Lower values make the bar more see-through."
        >
          <input
            type="range"
            min={30}
            max={100}
            step={5}
            value={settings.floatBarOpacity}
            disabled={saving || !settings.floatBarEnabled}
            onChange={(e) => set({ floatBarOpacity: Number(e.target.value) })}
            aria-label="Floating bar opacity"
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
