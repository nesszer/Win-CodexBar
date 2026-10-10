//! Pins every observable per-provider registry value: `all()` order, names,
//! cookie domains, brand colors, metadata, source support, and both alias
//! sets. Regenerate the dump with `CODEXBAR_UPDATE_REGISTRY_SNAPSHOT=1` only
//! for an intended change.

use std::fmt::Write as _;

use super::*;

const REGISTRY_SNAPSHOT: &str = include_str!("provider_registry_snapshot.txt");
const ALIASES_SNAPSHOT: &str = include_str!("provider_aliases_snapshot.txt");

fn registry_dump() -> String {
    let map = cli_name_map();
    let mut out = String::new();
    for &id in ProviderId::all() {
        let provider = instantiate_provider(id);
        let mut map_keys: Vec<&str> = map
            .iter()
            .filter(|(_, mapped)| **mapped == id)
            .map(|(key, _)| *key)
            .collect();
        map_keys.sort_unstable();
        writeln!(
            out,
            "{id:?} cli={} display={:?} cookie={:?} color={} deprecated={} blocking={:?}",
            id.cli_name(),
            id.display_name(),
            id.cookie_domain(),
            brand_color(id),
            id.is_deprecated(),
            id.blocking_quota_window_id(),
        )
        .unwrap();
        writeln!(out, "  map={map_keys:?}").unwrap();
        writeln!(out, "  metadata={:?}", provider.metadata()).unwrap();
        writeln!(
            out,
            "  sources={:?} oauth={} web={} cli={}",
            provider.available_sources(),
            provider.supports_oauth(),
            provider.supports_web(),
            provider.supports_cli(),
        )
        .unwrap();
    }
    out
}

#[test]
fn registry_matches_snapshot() {
    let actual = registry_dump();
    if std::env::var_os("CODEXBAR_UPDATE_REGISTRY_SNAPSHOT").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/core/provider_registry_snapshot.txt"
        );
        std::fs::write(path, &actual).unwrap();
    }
    assert_eq!(actual, REGISTRY_SNAPSHOT.replace('\r', ""));
}

fn alias_rows() -> Vec<(&'static str, &'static str)> {
    ALIASES_SNAPSHOT
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.split_once(" => ").expect("alias row"))
        .collect()
}

#[test]
fn every_snapshot_alias_resolves_to_its_provider() {
    let rows = alias_rows();
    assert_eq!(rows.len(), 241);
    for (alias, id) in rows {
        let resolved = ProviderId::from_cli_name(alias).map(|id| format!("{id:?}"));
        assert_eq!(resolved.as_deref(), Some(id), "alias {alias:?}");
    }
}
