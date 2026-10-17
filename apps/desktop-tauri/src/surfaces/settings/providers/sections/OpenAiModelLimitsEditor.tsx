import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import {
  draftsFromLimits,
  type LimitDraft,
  parseLimitDrafts,
} from "../../../../lib/openaiThroughput";
import type { ModelRateLimitSnapshot } from "../../../../types/bridge";

interface Props {
  /** The saved list; the editor resets to it whenever the reference changes. */
  limits: readonly ModelRateLimitSnapshot[];
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onSave: (limits: ModelRateLimitSnapshot[]) => void;
}

const EMPTY_DRAFT: LimitDraft = { model: "", tokensPerMinute: "", requestsPerMinute: "" };

/**
 * Manual per-model organization limits (upstream 0.75.0 #4495). Rows are checked
 * before save; a bad row keeps the editor as it is and shows why.
 */
export function OpenAiModelLimitsEditor({ limits, disabled, t, onSave }: Props) {
  const [drafts, setDrafts] = useState<LimitDraft[]>(() => draftsFromLimits(limits));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDrafts(draftsFromLimits(limits));
    setError(null);
  }, [limits]);

  const updateRow = (index: number, patch: Partial<LimitDraft>) => {
    setDrafts((current) =>
      current.map((row, rowIndex) => (rowIndex === index ? { ...row, ...patch } : row)),
    );
  };

  const save = () => {
    const result = parseLimitDrafts(drafts);
    if (!result.ok) {
      setError(t(result.error));
      return;
    }
    setError(null);
    onSave(result.limits);
  };

  return (
    <div className="openai-limits">
      <h4>{t("OpenAiLimitsTitle")}</h4>
      <div className="provider-detail-helper">{t("OpenAiLimitsHelp")}</div>
      {drafts.map((draft, index) => (
        <div className="openai-limits__row" key={index}>
          <label className="provider-detail-field">
            <span className="provider-detail-field__label">{t("OpenAiLimitModelLabel")}</span>
            <input
              className="provider-detail-field__input"
              value={draft.model}
              disabled={disabled}
              spellCheck={false}
              onChange={(event) => updateRow(index, { model: event.target.value })}
            />
          </label>
          <label className="provider-detail-field">
            <span className="provider-detail-field__label">{t("OpenAiLimitTpmLabel")}</span>
            <input
              className="provider-detail-field__input"
              inputMode="numeric"
              value={draft.tokensPerMinute}
              disabled={disabled}
              onChange={(event) => updateRow(index, { tokensPerMinute: event.target.value })}
            />
          </label>
          <label className="provider-detail-field">
            <span className="provider-detail-field__label">{t("OpenAiLimitRpmLabel")}</span>
            <input
              className="provider-detail-field__input"
              inputMode="numeric"
              value={draft.requestsPerMinute}
              disabled={disabled}
              onChange={(event) => updateRow(index, { requestsPerMinute: event.target.value })}
            />
          </label>
          <button
            type="button"
            className="credential-btn"
            disabled={disabled}
            onClick={() => setDrafts((current) => current.filter((_, i) => i !== index))}
          >
            {t("OpenAiLimitRemove")}
          </button>
        </div>
      ))}
      <div className="provider-detail-actions">
        <button
          type="button"
          className="credential-btn"
          disabled={disabled}
          onClick={() => setDrafts((current) => [...current, { ...EMPTY_DRAFT }])}
        >
          {t("OpenAiLimitAdd")}
        </button>
        <button
          type="button"
          className="credential-btn credential-btn--primary"
          disabled={disabled}
          onClick={save}
        >
          {t("OpenAiLimitSave")}
        </button>
      </div>
      {error && (
        <div className="provider-detail-error" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
