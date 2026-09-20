import { useEffect, useMemo, useState } from "react";
import {
  getProviderAzureApiVersion,
  setProviderAzureApiVersion,
} from "../../../../lib/tauri";

interface Props {
  providerId: string;
  disabled: boolean;
  onChanged: () => void;
}

const BUILT_IN_OPTIONS = [
  { value: "", label: "Default" },
  { value: "v1", label: "OpenAI-compatible v1" },
];

export function AzureApiVersionSection({
  providerId,
  disabled,
  onChanged,
}: Props) {
  const [value, setValue] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    setError(null);
    void getProviderAzureApiVersion(providerId)
      .then((next) => {
        if (!stale) setValue(next ?? "");
      })
      .catch((reason: unknown) => {
        if (!stale) setError(String(reason));
      });
    return () => {
      stale = true;
    };
  }, [providerId]);

  const options = useMemo(() => {
    if (!value || BUILT_IN_OPTIONS.some((option) => option.value === value)) {
      return BUILT_IN_OPTIONS;
    }
    return [...BUILT_IN_OPTIONS, { value, label: value }];
  }, [value]);

  const handleChange = async (next: string) => {
    if (next === value || busy || disabled) return;
    setBusy(true);
    setError(null);
    try {
      await setProviderAzureApiVersion(providerId, next);
      setValue(next);
      onChanged();
    } catch (reason: unknown) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="provider-detail-section provider-detail-region">
      <h4>Azure OpenAI API version</h4>
      <select
        className="provider-detail-select"
        value={value}
        disabled={disabled || busy}
        aria-label="Azure OpenAI API version"
        onChange={(event) => void handleChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <p className="provider-detail-helper">
        Default uses AZURE_OPENAI_API_VERSION, then 2024-10-21.
      </p>
      {error && <p className="provider-detail-error">{error}</p>}
    </section>
  );
}
