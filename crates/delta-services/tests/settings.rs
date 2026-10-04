use delta_services::save_llm_settings;

#[tokio::test]
async fn model_picker_uses_sorted_cached_models_when_provider_cannot_build() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let cache = temp.path().join("catalog.json");
    std::fs::write(&config, "[llm]\nprovider = 'unavailable'\n").unwrap();
    std::fs::write(&cache, r#"{"unavailable":{"fetched_at":0,"models":[{"id":"zeta"},{"id":"alpha"},{"id":"alpha"}]}}"#).unwrap();
    assert_eq!(
        delta_services::model_catalog(&config, &cache)
            .await
            .unwrap(),
        ["alpha", "zeta"]
    );
}

#[tokio::test]
async fn local_provider_setup_needs_no_key_and_preserves_routing() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let env = temp.path().join("keys.env");
    std::fs::write(
        &config,
        "[llm]\nmodel = 'keep'\n[llm.routing]\nreport = 'route'\n",
    )
    .unwrap();
    let setup = delta_services::ProviderSetup {
        name: "custom".into(),
        base_url: "http://localhost:11434/v1/chat/completions/".into(),
        api_key_env: "DELTA_TEST_UNSET_CUSTOM_KEY".into(),
        key: String::new(),
    };
    assert!(delta_services::connect_provider(&config, &env, &setup)
        .await
        .unwrap());
    let (_, cfg) = delta_core::config::load_config(&config).unwrap();
    assert_eq!(cfg.llm_base_url, "http://localhost:11434/v1");
    assert_eq!(cfg.llm_provider, "custom");
    assert_eq!(cfg.llm_model, "keep");
    assert_eq!(cfg.llm_routing["report"], "route");
    assert!(!env.exists());
    let mut invalid = setup.clone();
    invalid.base_url = "bad URL".into();
    assert!(delta_services::connect_provider(&config, &env, &invalid)
        .await
        .is_err());
    invalid.base_url = setup.base_url;
    invalid.key = "secret\nINJECTED=value".into();
    assert!(delta_services::connect_provider(&config, &env, &invalid)
        .await
        .is_err());
    assert!(!env.exists());
    assert!(!format!("{invalid:?}").contains("INJECTED"));
}

#[test]
fn source_setup_preserves_settings_and_rejects_invalid_input_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::fs::write(&path, "[plugins.sec_edgar]\nenabled = false\n[plugins.sec_edgar.scope]\ntags = ['growth']\n[llm]\nmodel = 'keep'\n").unwrap();
    let values =
        std::collections::BTreeMap::from([("contact".into(), " researcher@example.org ".into())]);
    delta_services::configure_data_provider(&path, "sec_edgar", &values, Some(&["us".into()]))
        .unwrap();
    let (_, cfg) = delta_core::config::load_config(&path).unwrap();
    assert_eq!(
        cfg.plugins["sec_edgar"]["contact"],
        "researcher@example.org"
    );
    assert_eq!(cfg.plugins["sec_edgar"]["enabled"], true);
    assert_eq!(cfg.plugins["sec_edgar"]["scope"]["tags"][0], "growth");
    assert_eq!(cfg.llm_model, "keep");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(delta_services::configure_data_provider(
        &path,
        "sec_edgar",
        &values,
        Some(&["missing".into()])
    )
    .is_err());
    let empty = std::collections::BTreeMap::from([("contact".into(), String::new())]);
    assert!(delta_services::configure_data_provider(&path, "sec_edgar", &empty, None).is_err());
    assert_eq!(saved, std::fs::read_to_string(&path).unwrap());
}

#[test]
fn runtime_universe_merges_targets_and_resolves_legacy_and_currency() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[markets.lse]
label = 'London'
currency = 'GBP'
yahoo_suffix = '.L'
[targets.first]
market = 'lse'
tickers = ['VOD']
tags = ['telecom']
[watchlists.second]
market = 'lse'
tickers = ['VOD']
tags = ['income']
[universe]
us = ['MSFT']
"#,
    )
    .unwrap();
    let universe = delta_services::configured_universe(&path).unwrap();
    assert_eq!(universe.len(), 2);
    let vod = universe.iter().find(|i| i.id == "LSE:VOD").unwrap();
    assert_eq!(vod.currency, "GBP");
    assert_eq!(vod.watchlists, ["first", "second"]);
    assert!(vod.tags.contains("telecom") && vod.tags.contains("income"));
    let microsoft = universe.iter().find(|i| i.id == "US:MSFT").unwrap();
    assert!(microsoft.watchlists.contains(&"universe_us".to_string()));
    // Shim entries stay hidden from the user's editable target list.
    assert!(!delta_services::target_specs(&path)
        .unwrap()
        .contains_key("universe_us"));
}

#[test]
fn model_settings_save_without_losing_routing_or_plugins() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::fs::write(&path,"[llm]\nprovider = 'openrouter'\nmodel = 'old'\n[llm.routing]\nreport = 'special'\n[plugins.rss]\nenabled = true\n").unwrap();
    save_llm_settings(&path, "openai", "new-model").unwrap();
    let (_, cfg) = delta_core::config::load_config(&path).unwrap();
    assert_eq!(cfg.llm_provider, "openai");
    assert_eq!(cfg.llm_model, "new-model");
    assert_eq!(cfg.llm_routing["report"], "special");
    assert_eq!(cfg.plugins["rss"]["enabled"], true);
    assert!(save_llm_settings(&path, "bogus", "new-model").is_err());
}
