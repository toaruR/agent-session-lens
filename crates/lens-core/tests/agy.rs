use lens_core::agy::{
    decode_timestamp, decode_varint, get_string_field, get_varint_field, parse_cwd_from_uri,
    AgyFile,
};
use lens_core::model::StatusValue;
use rusqlite::Connection;
use tempfile::NamedTempFile;

// --- Protobuf encoding helpers for synthetic test fixtures ---

fn encode_varint(n: u64) -> Vec<u8> {
    let mut v = Vec::new();
    let mut val = n;
    while val >= 0x80 {
        v.push(((val & 0x7f) as u8) | 0x80);
        val >>= 7;
    }
    v.push(val as u8);
    v
}

fn encode_tag(field_num: u32, wire_type: u8) -> Vec<u8> {
    encode_varint(((field_num as u64) << 3) | (wire_type as u64))
}

fn encode_varint_field(field_num: u32, val: u64) -> Vec<u8> {
    let mut out = encode_tag(field_num, 0);
    out.extend(encode_varint(val));
    out
}

fn encode_len_delim_field(field_num: u32, data: &[u8]) -> Vec<u8> {
    let mut out = encode_tag(field_num, 2);
    out.extend(encode_varint(data.len() as u64));
    out.extend_from_slice(data);
    out
}

fn encode_string_field(field_num: u32, text: &str) -> Vec<u8> {
    encode_len_delim_field(field_num, text.as_bytes())
}

fn encode_timestamp(sec: i64, nanos: u32) -> Vec<u8> {
    let mut out = encode_varint_field(1, sec as u64);
    if nanos > 0 {
        out.extend(encode_varint_field(2, nanos as u64));
    }
    out
}

#[test]
fn test_wire_decoder_primitives() {
    let mut data = Vec::new();
    data.extend(encode_varint_field(1, 42));
    data.extend(encode_string_field(2, "antigravity"));
    data.extend(encode_varint_field(3, 1000));

    assert_eq!(get_varint_field(&data, 1), Some(42));
    assert_eq!(get_string_field(&data, 2), Some("antigravity".into()));
    assert_eq!(get_varint_field(&data, 3), Some(1000));
    assert_eq!(get_varint_field(&data, 4), None);

    let mut offset = 0;
    assert_eq!(decode_varint(&encode_varint(12345), &mut offset), Some(12345));

    let ts_bytes = encode_timestamp(1786424672, 500000);
    let dt = decode_timestamp(&ts_bytes).unwrap();
    assert_eq!(dt.timestamp(), 1786424672);
}

#[test]
fn test_parse_cwd_from_uri() {
    assert_eq!(
        parse_cwd_from_uri("file:///d:/vagrant/harnesses/test-proj"),
        Some("d:\\vagrant\\harnesses\\test-proj".into())
    );
    assert_eq!(
        parse_cwd_from_uri("file:///d%3A/vagrant/harnesses/test-proj"),
        Some("d:\\vagrant\\harnesses\\test-proj".into())
    );
    assert_eq!(
        parse_cwd_from_uri("file:///home/user/project"),
        Some("/home/user/project".into())
    );
    assert_eq!(parse_cwd_from_uri(""), None);
}

#[test]
fn test_load_synthetic_session_waiting_input() {
    let tmp = NamedTempFile::new().unwrap();
    let conn = Connection::open(tmp.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE trajectory_meta (
            trajectory_id TEXT PRIMARY KEY,
            cascade_id TEXT,
            trajectory_type INTEGER,
            source INTEGER
        );
        CREATE TABLE trajectory_metadata_blob (
            id TEXT PRIMARY KEY DEFAULT 'main',
            data BLOB
        );
        CREATE TABLE steps (
            idx INTEGER PRIMARY KEY,
            step_type INTEGER NOT NULL,
            status INTEGER NOT NULL,
            has_subtrajectory NUMERIC NOT NULL DEFAULT 0,
            metadata BLOB,
            error_details BLOB,
            permissions BLOB,
            task_details BLOB,
            render_info BLOB,
            step_payload BLOB,
            step_format INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();

    conn.execute(
        "INSERT INTO trajectory_meta (trajectory_id, cascade_id, trajectory_type, source)
         VALUES ('traj-uuid-1', 'cascade-uuid-1', 4, 1)",
        [],
    )
    .unwrap();

    // trajectory_metadata_blob: field 7 = URI, field 2 = Timestamp
    let mut meta_blob = Vec::new();
    meta_blob.extend(encode_string_field(7, "file:///d:/vagrant/my-app"));
    meta_blob.extend(encode_len_delim_field(2, &encode_timestamp(1786400000, 0)));
    conn.execute(
        "INSERT INTO trajectory_metadata_blob (id, data) VALUES ('main', ?1)",
        [&meta_blob],
    )
    .unwrap();

    // Step 0: User prompt (step_type=14, status=3)
    let mut step0_meta = Vec::new();
    step0_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400001, 0)));
    let mut f19 = Vec::new();
    f19.extend(encode_string_field(2, "Please implement feature X"));
    let mut step0_payload = Vec::new();
    step0_payload.extend(encode_len_delim_field(19, &f19));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (0, 14, 3, ?1, ?2)",
        [&step0_meta, &step0_payload],
    )
    .unwrap();

    // Step 1: Assistant tool call (step_type=15, status=3)
    let mut step1_meta = Vec::new();
    step1_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400002, 0)));
    let mut f7 = Vec::new();
    f7.extend(encode_string_field(1, "call_101"));
    f7.extend(encode_string_field(2, "read_file"));
    f7.extend(encode_string_field(3, r#"{"path":"main.rs"}"#));
    let mut f20 = Vec::new();
    f20.extend(encode_len_delim_field(7, &f7));
    let mut step1_payload = Vec::new();
    step1_payload.extend(encode_len_delim_field(20, &f20));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (1, 15, 3, ?1, ?2)",
        [&step1_meta, &step1_payload],
    )
    .unwrap();

    // Step 2: Tool result (step_type=132, status=3)
    let mut step2_meta = Vec::new();
    step2_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400003, 0)));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (2, 132, 3, ?1, NULL)",
        [&step2_meta],
    )
    .unwrap();

    // Step 3: Assistant final response (step_type=15, status=3)
    let mut step3_meta = Vec::new();
    step3_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400004, 0)));
    let mut f20_final = Vec::new();
    f20_final.extend(encode_string_field(1, "Feature X has been implemented successfully."));
    let mut step3_payload = Vec::new();
    step3_payload.extend(encode_len_delim_field(20, &f20_final));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (3, 15, 3, ?1, ?2)",
        [&step3_meta, &step3_payload],
    )
    .unwrap();

    drop(conn);

    // Load with AgyFile
    let session = AgyFile::load(tmp.path()).unwrap();
    assert_eq!(session.native_id, "cascade-uuid-1");
    assert_eq!(session.cwd, Some("d:\\vagrant\\my-app".into()));
    assert_eq!(session.title, Some("Please implement feature X".into()));
    assert_eq!(session.last_user, Some("Please implement feature X".into()));
    assert_eq!(
        session.last_assistant,
        Some("Feature X has been implemented successfully.".into())
    );
    assert!(session.open_tools.is_empty());

    // Evaluates to WaitingInput when fresh
    let now = chrono::DateTime::from_timestamp(1786400010, 0).unwrap();
    let status = session.evaluate(now, None);
    assert_eq!(status.value, StatusValue::WaitingInput);
}

#[test]
fn test_load_synthetic_session_running_tool() {
    let tmp = NamedTempFile::new().unwrap();
    let conn = Connection::open(tmp.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE trajectory_meta (trajectory_id TEXT, cascade_id TEXT, trajectory_type INTEGER, source INTEGER);
        CREATE TABLE trajectory_metadata_blob (id TEXT, data BLOB);
        CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, metadata BLOB, step_payload BLOB);",
    )
    .unwrap();

    // Step 0: User prompt
    let mut step0_meta = Vec::new();
    step0_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400000, 0)));
    let mut f19 = Vec::new();
    f19.extend(encode_string_field(2, "Run tests"));
    let mut step0_payload = Vec::new();
    step0_payload.extend(encode_len_delim_field(19, &f19));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (0, 14, 3, ?1, ?2)",
        [&step0_meta, &step0_payload],
    )
    .unwrap();

    // Step 1: Assistant calls cargo test
    let mut step1_meta = Vec::new();
    step1_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400001, 0)));
    let mut f7 = Vec::new();
    f7.extend(encode_string_field(1, "call_999"));
    f7.extend(encode_string_field(2, "run_command"));
    f7.extend(encode_string_field(3, r#"{"cmd":"cargo test"}"#));
    let mut f20 = Vec::new();
    f20.extend(encode_len_delim_field(7, &f7));
    let mut step1_payload = Vec::new();
    step1_payload.extend(encode_len_delim_field(20, &f20));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (1, 15, 3, ?1, ?2)",
        [&step1_meta, &step1_payload],
    )
    .unwrap();

    // Step 2: Tool execution is RUNNING (status = 2)
    let mut step2_meta = Vec::new();
    step2_meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400002, 0)));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (2, 132, 2, ?1, NULL)",
        [&step2_meta],
    )
    .unwrap();

    drop(conn);

    let session = AgyFile::load(tmp.path()).unwrap();
    assert_eq!(session.open_tools.len(), 1);
    assert_eq!(session.open_tools[0].name, "run_command");

    let now = chrono::DateTime::from_timestamp(1786400005, 0).unwrap();
    let status = session.evaluate(now, None);
    assert_eq!(status.value, StatusValue::Running);
}

#[test]
fn test_load_synthetic_session_error_status() {
    let tmp = NamedTempFile::new().unwrap();
    let conn = Connection::open(tmp.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE trajectory_meta (trajectory_id TEXT, cascade_id TEXT, trajectory_type INTEGER, source INTEGER);
        CREATE TABLE trajectory_metadata_blob (id TEXT, data BLOB);
        CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, metadata BLOB, step_payload BLOB);",
    )
    .unwrap();

    // Step 0: User prompt
    let mut s0 = Vec::new();
    s0.extend(encode_len_delim_field(19, &encode_string_field(2, "Do something")));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (0, 14, 3, NULL, ?1)",
        [&s0],
    )
    .unwrap();

    // Step 1: Tool execution fails with status 7
    let mut meta = Vec::new();
    meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400002, 0)));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (1, 132, 7, ?1, NULL)",
        [&meta],
    )
    .unwrap();

    drop(conn);

    let session = AgyFile::load(tmp.path()).unwrap();
    let now = chrono::DateTime::from_timestamp(1786400005, 0).unwrap();
    let status = session.evaluate(now, None);
    assert_eq!(status.value, StatusValue::Error);
}

#[test]
fn test_load_synthetic_session_outside_of_project() {
    let tmp = NamedTempFile::new().unwrap();
    let conn = Connection::open(tmp.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE trajectory_meta (trajectory_id TEXT, cascade_id TEXT, trajectory_type INTEGER, source INTEGER);
        CREATE TABLE trajectory_metadata_blob (id TEXT, data BLOB);
        CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, metadata BLOB, step_payload BLOB);",
    )
    .unwrap();

    // field 18 = "outside-of-project", no field 7 or 1
    let mut meta_blob = Vec::new();
    meta_blob.extend(encode_string_field(18, "outside-of-project"));
    conn.execute(
        "INSERT INTO trajectory_metadata_blob (id, data) VALUES ('main', ?1)",
        [&meta_blob],
    )
    .unwrap();

    drop(conn);

    let session = AgyFile::load(tmp.path()).unwrap();
    assert_eq!(session.cwd, None);
}

#[test]
fn test_load_synthetic_session_legacy_step21() {
    let tmp = NamedTempFile::new().unwrap();
    let conn = Connection::open(tmp.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE trajectory_meta (trajectory_id TEXT, cascade_id TEXT, trajectory_type INTEGER, source INTEGER);
        CREATE TABLE trajectory_metadata_blob (id TEXT, data BLOB);
        CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, metadata BLOB, step_payload BLOB);",
    )
    .unwrap();

    // Step 0: User prompt
    let mut s0 = Vec::new();
    s0.extend(encode_len_delim_field(19, &encode_string_field(2, "Run ls")));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (0, 14, 3, NULL, ?1)",
        [&s0],
    )
    .unwrap();

    // Step 1: Legacy command (step_type=21, status=2)
    let mut f28 = Vec::new();
    f28.extend(encode_string_field(23, "ls -la"));
    let mut s1 = Vec::new();
    s1.extend(encode_len_delim_field(28, &f28));
    let mut meta = Vec::new();
    meta.extend(encode_len_delim_field(1, &encode_timestamp(1786400002, 0)));
    conn.execute(
        "INSERT INTO steps (idx, step_type, status, metadata, step_payload) VALUES (1, 21, 2, ?1, ?2)",
        [&meta, &s1],
    )
    .unwrap();

    drop(conn);

    let session = AgyFile::load(tmp.path()).unwrap();
    assert_eq!(session.open_tools.len(), 1);
    assert_eq!(session.open_tools[0].name, "run_command");
    assert_eq!(session.open_tools[0].input, "ls -la");
}
