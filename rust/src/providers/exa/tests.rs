#[cfg(test)]
mod tests {
    use crate::core::ProviderId;

    #[test]
    fn exa_id_is_registered() {
        let all = ProviderId::all();
        assert!(all.contains(&ProviderId::Exa));
    }

    #[test]
    fn exa_cli_name() {
        assert_eq!(ProviderId::Exa.cli_name(), "exa");
    }

    #[test]
    fn exa_display_name() {
        assert_eq!(ProviderId::Exa.display_name(), "Exa");
    }

    #[test]
    fn exa_from_cli_name() {
        assert_eq!(ProviderId::from_cli_name("exa"), Some(ProviderId::Exa));
    }

    #[test]
    fn exa_no_cookie_domain() {
        assert_eq!(ProviderId::Exa.cookie_domain(), None);
    }
}
