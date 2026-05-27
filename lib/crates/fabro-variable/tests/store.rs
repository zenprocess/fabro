use fabro_variable::{Error, VariableStore};

#[tokio::test]
async fn load_missing_file_returns_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = VariableStore::load(dir.path().join("variables.json"))
        .await
        .unwrap();

    assert!(store.list().await.is_empty());
}

#[tokio::test]
async fn set_get_list_and_reload_variables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("variables.json");
    let store = VariableStore::load(path.clone()).await.unwrap();

    let first = store
        .set("ZETA", "last", Some("Last variable"))
        .await
        .unwrap();
    let second = store.set("ALPHA", "", None).await.unwrap();

    assert_eq!(first.name, "ZETA");
    assert_eq!(first.value, "last");
    assert_eq!(first.description.as_deref(), Some("Last variable"));
    assert_eq!(second.value, "");
    assert_eq!(store.get("ZETA").await.unwrap().value, "last");
    assert_eq!(
        store
            .list()
            .await
            .into_iter()
            .map(|variable| variable.name)
            .collect::<Vec<_>>(),
        vec!["ALPHA", "ZETA"]
    );

    let reloaded = VariableStore::load(path).await.unwrap();
    assert_eq!(reloaded.get("ALPHA").await.unwrap().value, "");
    assert_eq!(
        reloaded.get("ZETA").await.unwrap().description.as_deref(),
        Some("Last variable")
    );
}

#[tokio::test]
async fn upsert_preserves_description_when_omitted_and_updates_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let store = VariableStore::load(dir.path().join("variables.json"))
        .await
        .unwrap();

    let created = store
        .set("DEPLOY_ENV", "staging", Some("Deployment target"))
        .await
        .unwrap();
    let preserved = store
        .set("DEPLOY_ENV", "production", None)
        .await
        .unwrap();
    let updated = store
        .set("DEPLOY_ENV", "preview", Some("Preview target"))
        .await
        .unwrap();

    assert_eq!(preserved.created_at, created.created_at);
    assert_eq!(preserved.description.as_deref(), Some("Deployment target"));
    assert_eq!(updated.description.as_deref(), Some("Preview target"));
    assert!(updated.updated_at >= preserved.updated_at);
}

#[tokio::test]
async fn update_existing_preserves_description_and_reports_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = VariableStore::load(dir.path().join("variables.json"))
        .await
        .unwrap();

    let created = store
        .set("DEPLOY_ENV", "staging", Some("Deployment target"))
        .await
        .unwrap();
    let updated = store
        .update_existing("DEPLOY_ENV", "production", None)
        .await
        .unwrap();

    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.value, "production");
    assert_eq!(updated.description.as_deref(), Some("Deployment target"));
    let err = store
        .update_existing("MISSING", "value", None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(name) if name == "MISSING"));
}

#[tokio::test]
async fn remove_deletes_variable_and_reports_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = VariableStore::load(dir.path().join("variables.json"))
        .await
        .unwrap();
    store
        .set("DEPLOY_ENV", "staging", None)
        .await
        .unwrap();

    store.remove("DEPLOY_ENV").await.unwrap();

    assert!(store.get("DEPLOY_ENV").await.is_none());
    let err = store.remove("DEPLOY_ENV").await.unwrap_err();
    assert!(matches!(err, Error::NotFound(name) if name == "DEPLOY_ENV"));
}

#[tokio::test]
async fn env_style_names_are_required() {
    let dir = tempfile::tempdir().unwrap();
    let store = VariableStore::load(dir.path().join("variables.json"))
        .await
        .unwrap();

    for invalid in ["", "1BAD", "bad-name", "BAD.NAME"] {
        let err = store.set(invalid, "value", None).await.unwrap_err();
        assert!(matches!(err, Error::InvalidName(name) if name == invalid));
    }

    store.set("_OK", "value", None).await.unwrap();
    store.set("OK_123", "value", None).await.unwrap();
}
