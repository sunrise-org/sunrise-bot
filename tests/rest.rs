use std::sync::{Arc, Mutex};

use sunrise_bot::{
    discord::{Api, ApiError, Response, rest::Rest},
    ids::Snowflake,
    kit::request::{ChannelPatch, Request, RolePatch},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Debug, Clone)]
struct Seen {
    request_line: String,
    authorization: Option<String>,
    body: String,
}

/// A server on localhost that answers with the scripted (status, body) pairs, one per request.
async fn serve(script: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        for (status, body) in script {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head, content_length) = loop {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    while buf.len() < end + 4 + len {
                        let n = sock.read(&mut chunk).await.unwrap();
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    break (head, len);
                }
            };
            let start = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            log.lock().unwrap().push(Seen {
                request_line: head.lines().next().unwrap().to_string(),
                authorization: head
                    .lines()
                    .find_map(|l| l.strip_prefix("authorization: ").map(str::to_string)),
                body: String::from_utf8_lossy(&buf[start..start + content_length]).to_string(),
            });
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (base, seen)
}

fn rest(base: String) -> Rest {
    Rest::with_base("secret-token".into(), Snowflake(42), base)
}

#[tokio::test]
async fn the_snapshot_reads_the_bot_the_guild_the_roles_and_the_channels() {
    let (base, seen) = serve(vec![
        (200, r#"{"id":"7","username":"Sunny"}"#),
        (200, r#"{"id":"42","name":"Sunrise","verification_level":2,"explicit_content_filter":2,"default_message_notifications":1,"features":[],"owner_id":"1"}"#),
        (200, r#"[{"id":"42","name":"@everyone","color":0,"permissions":"1024","position":0,"hoist":false,"mentionable":false,"managed":false}]"#),
        (200, r#"[{"id":"5","type":15,"name":"support","position":1,"available_tags":[{"id":"9","name":"crash","emoji_name":"💥","moderated":false}],"default_reaction_emoji":null,"permission_overwrites":[{"id":"7","type":1,"allow":"1024","deny":"0"}]}]"#),
    ])
    .await;
    let live = rest(base).snapshot().await.unwrap();
    assert_eq!(live.bot_id, Snowflake(7));
    assert_eq!(live.guild.name, "Sunrise");
    assert_eq!(live.roles[0].permissions.0, 1024);
    assert_eq!(live.channels[0].available_tags[0].name, "crash");
    let seen = seen.lock().unwrap();
    let paths: Vec<&str> = seen.iter().map(|s| s.request_line.as_str()).collect();
    assert_eq!(
        paths,
        [
            "GET /users/@me HTTP/1.1",
            "GET /guilds/42 HTTP/1.1",
            "GET /guilds/42/roles HTTP/1.1",
            "GET /guilds/42/channels HTTP/1.1"
        ]
    );
    assert!(
        seen.iter()
            .all(|s| s.authorization.as_deref() == Some("Bot secret-token"))
    );
}

#[tokio::test]
async fn a_rate_limited_write_is_sent_again() {
    let (base, seen) = serve(vec![
        (
            429,
            r#"{"message":"slow down","retry_after":0.01,"global":false}"#,
        ),
        (200, "{}"),
    ])
    .await;
    let patch = RolePatch {
        name: Some("Site".into()),
        ..RolePatch::default()
    };
    let r = rest(base)
        .send(Request::PatchRole {
            id: Snowflake(5),
            body: patch,
        })
        .await
        .unwrap();
    assert!(matches!(r, Response::Done));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].request_line, "PATCH /guilds/42/roles/5 HTTP/1.1");
    assert_eq!(seen[1].body, r#"{"name":"Site"}"#);
}

#[tokio::test]
async fn a_forbidden_answer_is_its_own_error_and_a_server_error_on_a_write_is_not_retried() {
    let (base, _) = serve(vec![(403, r#"{"message":"Missing Access","code":50001}"#)]).await;
    let err = rest(base)
        .send(Request::PatchChannel {
            id: Snowflake(1),
            body: ChannelPatch::default(),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::Forbidden(ref m) if m.contains("Missing Access")),
        "{err}"
    );

    let (base, seen) = serve(vec![(500, "oops"), (200, "{}")]).await;
    let err = rest(base)
        .send(Request::DeleteChannel { id: Snowflake(1) })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Status { status: 500, .. }));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn the_token_is_not_in_the_debug_output() {
    assert!(!format!("{:?}", rest("http://x".into())).contains("secret-token"));
}

#[tokio::test]
async fn messages_threads_and_posts_use_the_right_routes() {
    use sunrise_bot::kit::{
        message::{AllowedMentions, EmbedBody, MessageBody},
        request::ThreadBody,
    };
    let (base, seen) = serve(vec![
        (200, r#"[{"id":"9","author":{"id":"7","username":"Sunny"},"content":"","embeds":[{"type":"rich","title":"Rules","description":"d","color":7297241}],"components":[{"type":1,"components":[{"type":2,"custom_id":"role:site","label":"Site","style":2}]}],"pinned":false}]"#),
        (200, r#"{"threads":[{"id":"20","name":"a","parent_id":"5","owner_id":"7","flags":2,"thread_metadata":{"archived":false,"locked":true}},{"id":"21","name":"b","parent_id":"6","owner_id":"7","thread_metadata":{}}],"members":[]}"#),
        (200, r#"{"threads":[{"id":"22","name":"old","parent_id":"5","owner_id":"7","thread_metadata":{"archived":true,"locked":true}}],"has_more":false}"#),
        (200, r#"{"id":"30","author":{"id":"7"},"embeds":[],"components":[]}"#),
        (200, r#"{"id":"31","name":"post","parent_id":"5","owner_id":"7","thread_metadata":{}}"#),
    ])
    .await;
    let r = rest(base);
    let msgs = r.messages(Snowflake(3)).await.unwrap();
    assert_eq!(msgs[0].embeds[0].title.as_deref(), Some("Rules"));
    assert_eq!(
        msgs[0].components[0].components[0].custom_id.as_deref(),
        Some("role:site")
    );
    let threads = r.forum_threads(Snowflake(5)).await.unwrap();
    assert_eq!(
        threads.iter().map(|t| t.id.0).collect::<Vec<_>>(),
        [20, 22],
        "only this forum's posts, active and archived"
    );
    assert!(threads[0].pinned() && threads[1].thread_metadata.archived);

    let body = MessageBody {
        embeds: vec![EmbedBody {
            title: "T".into(),
            description: "D".into(),
            color: 1,
        }],
        components: vec![],
        allowed_mentions: AllowedMentions { parse: vec![] },
    };
    let Response::Message(m) = r
        .send(Request::CreateMessage {
            channel: Snowflake(3),
            body: body.clone(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(m.id, Snowflake(30));
    let Response::Thread(t) = r
        .send(Request::CreateThread {
            forum: Snowflake(5),
            body: ThreadBody {
                name: "post".into(),
                applied_tags: vec![Snowflake(8)],
                message: body,
            },
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(t.id, Snowflake(31));
    let seen = seen.lock().unwrap();
    let lines: Vec<&str> = seen.iter().map(|s| s.request_line.as_str()).collect();
    assert_eq!(
        lines,
        [
            "GET /channels/3/messages?limit=50 HTTP/1.1",
            "GET /guilds/42/threads/active HTTP/1.1",
            "GET /channels/5/threads/archived/public?limit=100 HTTP/1.1",
            "POST /channels/3/messages HTTP/1.1",
            "POST /channels/5/threads HTTP/1.1",
        ]
    );
    assert_eq!(
        seen[3].body,
        r#"{"embeds":[{"title":"T","description":"D","color":1}],"components":[],"allowed_mentions":{"parse":[]}}"#
    );
    assert_eq!(
        seen[4].body,
        r#"{"name":"post","applied_tags":["8"],"message":{"embeds":[{"title":"T","description":"D","color":1}],"components":[],"allowed_mentions":{"parse":[]}}}"#
    );
}
