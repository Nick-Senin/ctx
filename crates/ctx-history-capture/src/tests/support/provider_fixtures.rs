use super::*;

pub(in crate::tests) fn write_opencode_smoke_db(temp: &TempDir, malformed: bool) -> PathBuf {
    let path = temp.path().join(if malformed {
        "opencode-malformed.db"
    } else {
        "opencode.db"
    });
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
            "create table session (
                id text primary key, parent_id text, title text not null, directory text not null,
                model text, agent text, time_created integer not null, time_updated integer not null,
                tokens_input integer not null, tokens_output integer not null,
                tokens_reasoning integer not null, tokens_cache_read integer not null,
                tokens_cache_write integer not null
            );
            create table session_message (
                id text primary key, session_id text not null, type text not null, seq integer not null,
                time_created integer not null, time_updated integer not null, data text not null
            );",
        )
        .unwrap();
    conn.execute(
            "insert into session values (?1, null, 'root', '/workspace', '{\"id\":\"test\"}', 'build', 1782259200000, 1782259200000, 1, 1, 0, 0, 0)",
            ["opencode-root"],
        )
        .unwrap();
    conn.execute(
            "insert into session values (?1, ?2, 'child', '/workspace', '{\"id\":\"test\"}', 'scout', 1782259201000, 1782259201000, 1, 1, 0, 0, 0)",
            ["opencode-child", "opencode-root"],
        )
        .unwrap();
    conn.execute(
        "insert into session_message values (?1, ?2, 'user', 1, 1782259200000, 1782259200000, ?3)",
        [
            "msg-user",
            "opencode-root",
            "{\"time\":{\"created\":1782259200000},\"text\":\"inspect\"}",
        ],
    )
    .unwrap();
    conn.execute(
            "insert into session_message values (?1, ?2, 'assistant', 2, 1782259201000, 1782259201000, ?3)",
            ["msg-assistant", "opencode-root", "{\"time\":{\"created\":1782259201000},\"content\":[{\"type\":\"tool\",\"name\":\"bash\"}]}"],
        )
        .unwrap();
    let child_data = if malformed {
        "{\"time\":{\"created\":1782259202000},\"text\":"
    } else {
        "{\"time\":{\"created\":1782259202000},\"text\":\"child done\"}"
    };
    conn.execute(
            "insert into session_message values (?1, ?2, 'assistant', 1, 1782259202000, 1782259202000, ?3)",
            ["msg-child", "opencode-child", child_data],
        )
        .unwrap();
    path
}

pub(in crate::tests) fn write_hermes_smoke_db(temp: &TempDir) -> PathBuf {
    let path = temp.path().join("hermes-state.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table sessions (
                id text primary key,
                source text not null,
                started_at real not null
            );
            create table messages (
                id integer primary key autoincrement,
                session_id text not null,
                role text not null,
                content text,
                timestamp real not null,
                active integer not null default 1,
                compacted integer not null default 0
            );",
    )
    .unwrap();
    conn.execute(
        "insert into sessions values (?1, 'acp', 1782259200.0)",
        ["hermes-root"],
    )
    .unwrap();
    conn.execute(
            "insert into messages (session_id, role, content, timestamp) values (?1, 'user', 'bad timestamp', 1782259201.0)",
            ["hermes-root"],
        )
        .unwrap();
    conn.execute(
            "insert into messages (session_id, role, content, timestamp) values (?1, 'assistant', 'good timestamp', 1782259202.0)",
            ["hermes-root"],
        )
        .unwrap();
    path
}

pub(in crate::tests) fn write_nanoclaw_smoke_project(temp: &TempDir, query: &str) -> PathBuf {
    let root = temp.path().join("native-nanoclaw");
    let data = root.join("data");
    let session_dir = data.join("v2-sessions/ag-1/session-1");
    fs::create_dir_all(&session_dir).unwrap();
    let central = Connection::open(data.join("v2.db")).unwrap();
    central
        .execute_batch(
            "create table agent_groups (
                id text primary key,
                name text,
                folder text,
                agent_provider text
            );
            create table messaging_groups (
                id text primary key,
                channel_type text,
                platform_id text,
                instance text,
                name text
            );
            create table sessions (
                id text primary key,
                agent_group_id text not null,
                messaging_group_id text,
                thread_id text,
                agent_provider text,
                status text,
                container_status text,
                last_active integer,
                created_at integer
            );",
        )
        .unwrap();
    central
        .execute(
            "insert into agent_groups values ('ag-1', 'Personal', '/workspace/nanoclaw', 'codex')",
            [],
        )
        .unwrap();
    central
        .execute(
            "insert into messaging_groups values ('mg-1', 'telegram', 'chat-1', 'default', 'DM')",
            [],
        )
        .unwrap();
    central
        .execute(
            "insert into sessions values (
                'session-1', 'ag-1', 'mg-1', 'thread-1', 'codex', 'active',
                'running', 1782259202000, 1782259200000
            )",
            [],
        )
        .unwrap();
    let inbound = Connection::open(session_dir.join("inbound.db")).unwrap();
    inbound
        .execute_batch(
            "create table messages_in (
                id text primary key,
                seq integer,
                kind text,
                timestamp integer,
                status text,
                trigger text,
                platform_id text,
                channel_type text,
                thread_id text,
                content text,
                source_session_id text,
                on_wake integer
            );",
        )
        .unwrap();
    inbound
        .execute(
            "insert into messages_in values (
                'in-1', 1, 'chat', 1782259201000, 'done', 'message',
                'chat-1', 'telegram', 'thread-1', ?1, null, 0
            )",
            [json!({"text": query}).to_string()],
        )
        .unwrap();
    let outbound = Connection::open(session_dir.join("outbound.db")).unwrap();
    outbound
        .execute_batch(
            "create table messages_out (
                id text primary key,
                seq integer,
                in_reply_to text,
                timestamp integer,
                kind text,
                platform_id text,
                channel_type text,
                thread_id text,
                content text
            );",
        )
        .unwrap();
    outbound
        .execute(
            "insert into messages_out values (
                'out-1', 2, 'in-1', 1782259202000, 'chat',
                'chat-1', 'telegram', 'thread-1', ?1
            )",
            [json!({"text": "nanoclaw native import ok"}).to_string()],
        )
        .unwrap();
    root
}

pub(in crate::tests) fn write_opencode_session_message_without_seq_db(temp: &TempDir) -> PathBuf {
    let path = temp.path().join("opencode-no-seq.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table session (
                id text primary key, title text not null, directory text not null,
                time_created integer not null, time_updated integer not null
            );
            create table session_message (
                id text primary key, session_id text not null, type text not null,
                time_created integer not null, time_updated integer not null, data text not null
            );",
    )
    .unwrap();
    conn.execute(
        "insert into session values (?1, 'no seq', '/workspace', 1782259200000, 1782259200000)",
        ["opencode-no-seq"],
    )
    .unwrap();
    conn.execute(
        "insert into session_message values (?1, ?2, 'user', 1782259200000, 1782259200000, ?3)",
        [
            "msg-no-seq-user",
            "opencode-no-seq",
            "{\"time\":{\"created\":1782259200000},\"text\":\"first no seq\"}",
        ],
    )
    .unwrap();
    conn.execute(
            "insert into session_message values (?1, ?2, 'assistant', 1782259201000, 1782259201000, ?3)",
            [
                "msg-no-seq-assistant",
                "opencode-no-seq",
                "{\"time\":{\"created\":1782259201000},\"text\":\"second no seq\"}",
            ],
        )
        .unwrap();
    path
}

pub(in crate::tests) fn write_opencode_current_schema_db(
    temp: &TempDir,
    with_message: bool,
) -> PathBuf {
    let path = temp.path().join(if with_message {
        "opencode-current-message.db"
    } else {
        "opencode-current-empty.db"
    });
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table session (
                id text primary key,
                project_id text not null,
                parent_id text,
                slug text not null,
                directory text not null,
                title text not null,
                version text not null,
                share_url text,
                summary_additions integer,
                summary_deletions integer,
                summary_files integer,
                summary_diffs text,
                revert text,
                permission text,
                time_created integer not null,
                time_updated integer not null,
                time_compacting integer,
                time_archived integer,
                workspace_id text
            );
            create table session_entry (
                id text primary key,
                session_id text not null,
                type text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );
            create table message (
                id text primary key,
                session_id text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );
            create table part (
                id text primary key,
                message_id text not null,
                session_id text not null,
                type text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );",
    )
    .unwrap();

    if with_message {
        conn.execute(
            "insert into session (
                    id, project_id, parent_id, slug, directory, title, version, permission,
                    time_created, time_updated
                ) values (?1, 'project-1', null, 'current-root', '/workspace', 'current root',
                    '0.8.0', 'default', 1782259200000, 1782259200000)",
            ["current-root"],
        )
        .unwrap();
        conn.execute(
                "insert into message values (?1, ?2, 1782259200000, 1782259200000, ?3)",
                [
                    "current-message-1",
                    "current-root",
                    "{\"role\":\"user\",\"time\":{\"created\":1782259200000},\"text\":\"legacy hello\"}",
                ],
            )
            .unwrap();
    }

    path
}

pub(in crate::tests) fn write_opencode_message_part_db(
    temp: &TempDir,
    name: &str,
    session_id: &str,
    oracle_text: &str,
) -> PathBuf {
    let path = temp.path().join(name);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table session (
                id text primary key,
                project_id text not null,
                parent_id text,
                slug text not null,
                directory text not null,
                title text not null,
                version text not null,
                share_url text,
                summary_additions integer,
                summary_deletions integer,
                summary_files integer,
                summary_diffs text,
                revert text,
                permission text,
                time_created integer not null,
                time_updated integer not null,
                time_compacting integer,
                time_archived integer,
                workspace_id text
            );
            create table message (
                id text primary key,
                session_id text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );
            create table part (
                id text primary key,
                message_id text not null,
                session_id text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );",
    )
    .unwrap();
    conn.execute(
        "insert into session (
                id, project_id, parent_id, slug, directory, title, version, permission,
                time_created, time_updated
            ) values (?1, 'project-1', null, ?1, '/workspace', 'part root', '0.8.0',
                'default', 1782259200000, 1782259200000)",
        [session_id],
    )
    .unwrap();
    conn.execute(
        "insert into message values (?1, ?2, 1782259201000, 1782259201000, ?3)",
        [
            "part-message",
            session_id,
            &json!({
                "role": "assistant",
                "time": { "created": 1782259201000_i64 },
                "providerID": "anthropic",
                "modelID": "claude-sonnet-4"
            })
            .to_string(),
        ],
    )
    .unwrap();
    conn.execute(
        "insert into part values (?1, 'part-message', ?2, 1782259201001, 1782259201001, ?3)",
        [
            "part-text",
            session_id,
            &json!({
                "type": "text",
                "text": oracle_text
            })
            .to_string(),
        ],
    )
    .unwrap();
    conn.execute(
        "insert into part values (?1, 'part-message', ?2, 1782259201002, 1782259201002, ?3)",
        [
            "part-tool",
            session_id,
            &json!({
                "type": "tool",
                "tool": "write_file",
                "state": {
                    "status": "completed",
                    "metadata": {
                        "exit": 0,
                        "outputPath": "src/tool_arg_should_not_touch.txt",
                        "truncated": false
                    }
                },
                "input": { "path": "src/tool_arg_should_not_touch.txt" }
            })
            .to_string(),
        ],
    )
    .unwrap();
    conn.execute(
        "insert into part values (?1, 'part-message', ?2, 1782259201003, 1782259201003, ?3)",
        [
            "part-patch",
            session_id,
            &json!({
                "type": "patch",
                "status": "completed",
                "path": "src/opencode_part.txt",
                "files": ["src/opencode_part_from_files.txt"],
                "patch": "*** Begin Patch\n*** Update File: src/opencode_part.txt\n@@\n-raw-opencode-patch-needle\n+new\n*** End Patch"
            })
            .to_string(),
        ],
    )
    .unwrap();
    path
}

pub(in crate::tests) fn write_opencode_session_message_metadata_with_legacy_message_db(
    temp: &TempDir,
) -> PathBuf {
    write_opencode_strict_real_content_db(
        temp,
        "opencode-session-message-metadata-legacy.db",
        true,
        false,
        true,
        false,
    )
}

pub(in crate::tests) fn write_opencode_session_message_malformed_with_legacy_message_db(
    temp: &TempDir,
) -> PathBuf {
    let path = write_opencode_strict_real_content_db(
        temp,
        "opencode-session-message-malformed-legacy.db",
        true,
        false,
        true,
        false,
    );
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "update session_message set data = ?1 where id = 'metadata-session-message'",
        ["{\"time\":{\"created\":1782259200000},\"text\":"],
    )
    .unwrap();
    path
}

pub(in crate::tests) fn write_opencode_session_message_metadata_bad_seq_with_legacy_message_db(
    temp: &TempDir,
) -> PathBuf {
    let path = write_opencode_session_message_metadata_with_legacy_message_db(temp);
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "update session_message set seq = -1 where id = 'metadata-session-message'",
        [],
    )
    .unwrap();
    path
}

pub(in crate::tests) fn write_opencode_session_entry_metadata_with_legacy_message_db(
    temp: &TempDir,
) -> PathBuf {
    write_opencode_strict_real_content_db(
        temp,
        "opencode-session-entry-metadata-legacy.db",
        false,
        true,
        true,
        false,
    )
}

pub(in crate::tests) fn write_opencode_all_metadata_db(temp: &TempDir, name: &str) -> PathBuf {
    write_opencode_strict_real_content_db(temp, name, true, true, false, true)
}

pub(in crate::tests) fn write_opencode_tool_only_db(temp: &TempDir, name: &str) -> PathBuf {
    let path = write_opencode_strict_real_content_db(temp, name, false, false, false, false);
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "insert into session_message values (
                'tool-only-session-message', 'strict-root', 'assistant', 1,
                1782259200000, 1782259200000, ?1
            )",
        ["{\"time\":{\"created\":1782259200000},\"content\":[{\"type\":\"tool\",\"name\":\"bash\",\"input\":{\"command\":\"true\"}}]}"],
    )
    .unwrap();
    path
}

fn write_opencode_strict_real_content_db(
    temp: &TempDir,
    name: &str,
    session_message_metadata: bool,
    session_entry_metadata: bool,
    legacy_real_message: bool,
    legacy_metadata_message: bool,
) -> PathBuf {
    let path = temp.path().join(name);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table session (
                id text primary key,
                title text not null,
                directory text not null,
                time_created integer not null,
                time_updated integer not null
            );
            create table session_message (
                id text primary key,
                session_id text not null,
                type text not null,
                seq integer not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );
            create table session_entry (
                id text primary key,
                session_id text not null,
                type text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );
            create table message (
                id text primary key,
                session_id text not null,
                time_created integer not null,
                time_updated integer not null,
                data text not null
            );",
    )
    .unwrap();
    conn.execute(
        "insert into session values (
                'strict-root', 'strict root', '/workspace', 1782259200000, 1782259200000
            )",
        [],
    )
    .unwrap();
    if session_message_metadata {
        conn.execute(
                "insert into session_message values (
                    'metadata-session-message', 'strict-root', 'model_change', 1,
                    1782259200000, 1782259200000, ?1
                )",
                ["{\"time\":{\"created\":1782259200000},\"provider\":\"openai\",\"model\":\"metadata-only\"}"],
            )
            .unwrap();
    }
    if session_entry_metadata {
        conn.execute(
            "insert into session_entry values (
                    'metadata-session-entry', 'strict-root', 'label',
                    1782259200001, 1782259200001, ?1
                )",
            ["{\"time\":{\"created\":1782259200001},\"label\":\"metadata-only\"}"],
        )
        .unwrap();
    }
    if legacy_real_message {
        conn.execute(
            "insert into message values (
                    'legacy-real-message', 'strict-root', 1782259200002, 1782259200002, ?1
                )",
            ["{\"role\":\"user\",\"time\":{\"created\":1782259200002},\"text\":\"legacy fallback prompt\"}"],
        )
        .unwrap();
    }
    if legacy_metadata_message {
        conn.execute(
            "insert into message values (
                    'legacy-metadata-message', 'strict-root', 1782259200002, 1782259200002, ?1
                )",
            ["{\"type\":\"model_change\",\"time\":{\"created\":1782259200002},\"model\":\"metadata-only-legacy\"}"],
        )
        .unwrap();
    }
    path
}

pub(in crate::tests) fn write_opencode_future_incomplete_schema_db(temp: &TempDir) -> PathBuf {
    let path = temp.path().join("opencode-future-incomplete.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "create table session (
                id text primary key,
                project_id text not null,
                slug text not null,
                directory text not null,
                title text not null,
                version text not null,
                time_created integer not null,
                time_updated integer not null
            );
            create table message (
                id text primary key,
                session_id text not null,
                time_created integer not null,
                time_updated integer not null
            );",
    )
    .unwrap();
    conn.execute(
        "insert into session (
                id, project_id, slug, directory, title, version, time_created, time_updated
            ) values ('future-root', 'project-1', 'future-root', '/workspace', 'future root',
                '0.9.0', 1782259200000, 1782259200000)",
        [],
    )
    .unwrap();
    conn.execute(
        "insert into message values ('future-message-1', 'future-root', 1782259200000,
                1782259200000)",
        [],
    )
    .unwrap();
    path
}
