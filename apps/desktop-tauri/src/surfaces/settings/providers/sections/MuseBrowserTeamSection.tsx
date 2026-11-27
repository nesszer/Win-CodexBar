import { useEffect, useMemo, useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import {
  getProviderWorkspaceId,
  setProviderWorkspaceId,
} from "../../../../lib/tauri";
import type { ProviderDisplayDetailSnapshot } from "../../../../types/bridge";

/** Detail-row id prefix the Muse provider uses for each listed browser team. */
export const MUSE_BROWSER_TEAM_ROW_PREFIX = "browser-team-";

interface Props {
  providerId: string;
  details: ProviderDisplayDetailSnapshot[] | undefined;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChanged: () => void;
}

interface TeamOption {
  id: string;
  label: string;
}

function listedTeams(details: ProviderDisplayDetailSnapshot[] | undefined): TeamOption[] {
  return (details ?? [])
    .filter((row) => row.id.startsWith(MUSE_BROWSER_TEAM_ROW_PREFIX))
    .map((row) => ({ id: row.value, label: `${row.title} (${row.value})` }));
}

/**
 * Picker for the dev.meta.ai team whose quota Muse Code reads when the
 * device login reports none. The choice is stored as the provider workspace
 * value; an empty choice never falls back to the first listed team.
 */
export function MuseBrowserTeamSection({
  providerId,
  details,
  disabled,
  t,
  onChanged,
}: Props) {
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    setBusy(true);
    setError(null);
    void getProviderWorkspaceId(providerId)
      .then((value) => {
        if (!stale) setSelected(value ?? "");
      })
      .catch((reason: unknown) => {
        if (!stale) setError(String(reason));
      })
      .finally(() => {
        if (!stale) setBusy(false);
      });
    return () => {
      stale = true;
    };
  }, [providerId]);

  const teams = useMemo(() => listedTeams(details), [details]);
  const options = useMemo(
    () =>
      !selected || teams.some((team) => team.id === selected)
        ? teams
        : [
            ...teams,
            {
              id: selected,
              label: `${selected} (${t("MuseBrowserTeamUnavailable")})`,
            },
          ],
    [teams, selected, t],
  );

  const handleChange = async (next: string) => {
    if (next === selected || busy || disabled) return;
    setBusy(true);
    setError(null);
    try {
      await setProviderWorkspaceId(providerId, next);
      setSelected(next);
      onChanged();
    } catch (reason: unknown) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="provider-detail-section provider-detail-region">
      <h4>{t("MuseBrowserTeamTitle")}</h4>
      <select
        className="provider-detail-select"
        value={selected}
        disabled={disabled || busy}
        aria-label={t("MuseBrowserTeamTitle")}
        onChange={(event) => void handleChange(event.target.value)}
      >
        <option value="">{t("MuseBrowserTeamChoose")}</option>
        {options.map((team) => (
          <option key={team.id} value={team.id}>
            {team.label}
          </option>
        ))}
      </select>
      <p className="provider-detail-helper">
        {t(teams.length === 0 ? "MuseBrowserTeamNoTeams" : "MuseBrowserTeamHelp")}
      </p>
      {error && <p className="provider-detail-error">{error}</p>}
    </section>
  );
}
