//! Config contracts: the shipped example must parse, defaults must hold,
//! and typos must be rejected loudly (arr-style).

use narratarr::config::{Config, EXAMPLE};

#[test]
fn example_config_parses() {
    let cfg: Config = toml::from_str(EXAMPLE).expect("EXAMPLE must always parse");
    assert_eq!(cfg.general.interval.as_deref(), Some("6h"));
    assert!(!cfg.general.apply, "example must ship in dry-run");
    assert_eq!(cfg.audiobookshelf.unwrap().library, "Books");
    assert_eq!(cfg.listenarr.unwrap().api_key, "CHANGEME");
}

#[test]
fn minimal_config_gets_defaults() {
    let cfg: Config = toml::from_str("").unwrap();
    assert_eq!(cfg.general.language, "en");
    assert!(!cfg.general.apply);
    assert_eq!(cfg.general.limit, 0);
    assert!(cfg.audiobookshelf.is_none());
}

#[test]
fn unknown_keys_are_rejected() {
    // a typo must be an error, not a silently-ignored setting
    assert!(toml::from_str::<Config>("[general]\nintervall = \"6h\"\n").is_err());
    assert!(toml::from_str::<Config>("[listenar]\nurl = \"x\"\napi_key = \"y\"\n").is_err());
}
