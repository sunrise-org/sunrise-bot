use sunrise_bot::{
    config::Key,
    discord::executor::Ledger,
    ids::Snowflake,
    kit::plan::{ObjectKind, Origin},
    store::Store,
};

fn key(s: &str) -> Key {
    Key::new(s).unwrap()
}

#[tokio::test]
async fn ids_are_kept_by_kind_and_key() {
    let store = Store::memory().await.unwrap();
    store
        .record(
            ObjectKind::Channel,
            &key("general"),
            Snowflake(5),
            Origin::Created,
        )
        .await
        .unwrap();
    store
        .record(
            ObjectKind::Role,
            &key("general"),
            Snowflake(6),
            Origin::Adopted,
        )
        .await
        .unwrap();
    let known = store.load().await.unwrap();
    assert_eq!(
        known.get(ObjectKind::Channel, &key("general")).unwrap().id,
        Snowflake(5)
    );
    assert_eq!(
        known.get(ObjectKind::Role, &key("general")).unwrap().origin,
        Origin::Adopted
    );
    assert_eq!(known.iter().count(), 2);
}

#[tokio::test]
async fn recording_again_keeps_who_made_it_unless_the_id_changed() {
    let store = Store::memory().await.unwrap();
    let k = key("rules");
    store
        .record(ObjectKind::Channel, &k, Snowflake(1), Origin::Created)
        .await
        .unwrap();
    store
        .record(ObjectKind::Channel, &k, Snowflake(1), Origin::Adopted)
        .await
        .unwrap();
    assert_eq!(
        store
            .load()
            .await
            .unwrap()
            .get(ObjectKind::Channel, &k)
            .unwrap()
            .origin,
        Origin::Created
    );
    store
        .record(ObjectKind::Channel, &k, Snowflake(2), Origin::Adopted)
        .await
        .unwrap();
    let now = store
        .load()
        .await
        .unwrap()
        .get(ObjectKind::Channel, &k)
        .unwrap();
    assert_eq!((now.id, now.origin), (Snowflake(2), Origin::Adopted));
    store.forget(ObjectKind::Channel, &k).await.unwrap();
    assert!(
        store
            .load()
            .await
            .unwrap()
            .get(ObjectKind::Channel, &k)
            .is_none()
    );
}

#[tokio::test]
async fn the_file_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("sunrise-bot-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bot.sqlite");
    Store::open(&path)
        .await
        .unwrap()
        .record(
            ObjectKind::Category,
            &key("start"),
            Snowflake(9),
            Origin::Created,
        )
        .await
        .unwrap();
    let again = Store::open(&path).await.unwrap().load().await.unwrap();
    assert_eq!(
        again.get(ObjectKind::Category, &key("start")).unwrap().id,
        Snowflake(9)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
