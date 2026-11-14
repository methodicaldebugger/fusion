//! End-to-end tests for the 0.3 standard library, test runner and documentation generator.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_file(source: &str) -> std::path::PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("fusion_stdlib_test_{}_{}.fusion", std::process::id(), id));
    fs::write(&path, source).expect("write temporary Fusion source");
    path
}

fn fusion() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fusion"))
}

fn run(source: &str) -> Vec<String> {
    let path = temp_file(source);
    let output = fusion().arg("run").arg(&path).output().expect("run Fusion CLI");
    let _ = fs::remove_file(&path);
    assert!(output.status.success(), "CLI failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).lines().map(|l| l.to_string()).collect()
}

fn run_with(extra: &[&str], source: &str) -> std::process::Output {
    let path = temp_file(source);
    let output = fusion().arg("run").arg(&path).args(extra).output().expect("run Fusion CLI");
    let _ = fs::remove_file(&path);
    output
}

fn lines(output: &std::process::Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout).lines().map(|l| l.to_string()).collect()
}

#[test]
fn regex_and_string_helpers() {
    let out = run("main:\n    print(regex_is_match(\"^[a-z]+$\", \"hello\"))\n    print(regex_replace(\"[0-9]+\", \"a1b22\", \"#\"))\n    print(regex_split(\"[ ]+\", \"a  b c\"))\n    print(str_slice(\"hello\", 1, 3))\n    print(str_find(\"hello\", \"ll\"))\n    print(utf8_valid([104, 105]))\n    print(utf8_valid([255, 254]))\n");
    assert_eq!(out, vec!["true", "a#b#", "[a, b, c]", "el", "2", "true", "false"]);
}

#[test]
fn json_and_toml() {
    let out = run("main:\n    print(json_stringify(json_parse(\"[1, 2, 3]\")))\n    print(json_stringify(hashmap(\"a\", 1)))\n    print(toml_stringify(toml_parse(\"a = 1\")))\n");
    assert_eq!(out[0], "[1,2,3]");
    assert_eq!(out[1], "{\"a\":1}");
    assert_eq!(out[2], "a = 1");
}

#[test]
fn hashing_hmac_and_randomness() {
    let out = run("main:\n    print(sha256(\"abc\"))\n    print(hmac_sha256(\"Jefe\", \"what do ya want for nothing?\"))\n    print(str_char_count(random_bytes_hex(8)))\n    print(str_char_count(uuid4()))\n");
    assert_eq!(out[0], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(out[1], "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    assert_eq!(out[2], "16");
    assert_eq!(out[3], "36");
}

#[test]
fn dates_with_offsets() {
    let out = run("main:\n    t = date_from_parts(2026, 10, 31, 12, 30, 45, \"+02:00\")\n    print(t)\n    print(date_format(t, \"%Y-%m-%d %H:%M:%S\", \"UTC\"))\n    print(date_format(1793449845, \"%A %d %B %Y\", \"Z\"))\n    print(date_parse(\"2026-10-31T12:30:45Z\"))\n    print(days_in_month(2024, 2))\n");
    assert_eq!(out, vec!["1793442645", "2026-10-31 10:30:45", "Saturday 31 October 2026", "Some(1793449845)", "29"]);
}

#[test]
fn tasks_channels_and_parallel_map() {
    let source = "fn square(x: num) -> num:\n    return x * x\n\nfn producer(ch: num, n: num):\n    channel_send(ch, n * 2)\n\nmain:\n    ids = [task_spawn(\"square\", [3]), task_spawn(\"square\", [4])]\n    print(task_join_all(ids))\n    print(parallel_map(\"square\", [1, 2, 3, 4]))\n    ch = channel_new(0)\n    t = task_spawn(\"producer\", [ch, 21])\n    print(channel_recv(ch))\n    r = task_join(t)\n    c = counter_new(5)\n    print(counter_add(c, 2))\n";
    assert_eq!(run(source), vec!["[9, 16]", "[1, 4, 9, 16]", "Some(42)", "7"]);
}

#[test]
fn file_and_path_helpers() {
    let dir = std::env::temp_dir().join(format!("fusion_stdlib_fs_{}", std::process::id()));
    let dir_text = dir.to_string_lossy().replace('\\', "/");
    let source = format!(
        "main:\n    create_dir(\"{d}/sub\")\n    append_file(\"{d}/a.txt\", \"one\\ntwo\\n\")\n    print(read_lines(\"{d}/a.txt\"))\n    print(file_size(\"{d}/a.txt\"))\n    copy_file(\"{d}/a.txt\", \"{d}/sub/b.txt\")\n    print(walk_dir(\"{d}\"))\n    print(path_is_file(\"{d}/a.txt\"))\n    remove_dir(\"{d}\")\n    print(path_is_dir(\"{d}\"))\n",
        d = dir_text
    );
    let out = run(&source);
    assert_eq!(out[0], "[one, two]");
    assert_eq!(out[1], "8");
    assert!(out[2].starts_with("[a.txt, sub"), "unexpected walk_dir output: {}", out[2]);
    assert_eq!(out[3], "true");
    assert_eq!(out[4], "false");
}

#[test]
fn test_runner_reports_pass_fail_and_ignored() {
    let source = "#[test]\nfn addition_works():\n    assert_eq(1 + 1, 2)\n\n#[test]\n#[should_panic]\nfn failing_assert_panics():\n    assert_eq(1, 2)\n\n#[test]\n#[ignore]\nfn skipped():\n    test_fail(\"never runs\")\n\nmain:\n    print(\"not run by fusion test\")\n";
    let path = temp_file(source);
    let output = fusion().arg("test").arg(&path).output().expect("run fusion test");
    let _ = fs::remove_file(&path);
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "stdout: {}\nstderr: {}", text, String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("test addition_works ... ok"), "{}", text);
    assert!(text.contains("test failing_assert_panics ... ok"), "{}", text);
    assert!(text.contains("test skipped ... ignored"), "{}", text);
    assert!(text.contains("3 passed") || text.contains("2 passed; 0 failed; 1 ignored"), "{}", text);
    assert!(!text.contains("not run by fusion test"));
}

#[test]
fn test_runner_fails_with_nonzero_exit() {
    let path = temp_file("#[test]\nfn broken():\n    assert_eq(1, 2)\n\nmain:\n    print(1)\n");
    let output = fusion().arg("test").arg(&path).output().expect("run fusion test");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("test broken ... FAILED"), "{}", text);
    assert!(text.contains("left: 1"), "{}", text);
}

#[test]
fn doc_generator_emits_markdown_and_html() {
    let source = "//! Math helpers.\n\n/// Adds two numbers.\npub fn add(a: num, b: num) -> num:\n    return a + b\n\nmain:\n    print(add(1, 2))\n";
    let path = temp_file(source);
    let md = fusion().arg("doc").arg(&path).output().expect("run fusion doc");
    let html = fusion().arg("doc").arg(&path).arg("--html").output().expect("run fusion doc --html");
    let _ = fs::remove_file(&path);
    let md = String::from_utf8_lossy(&md.stdout).to_string();
    let html = String::from_utf8_lossy(&html.stdout).to_string();
    assert!(md.contains("Math helpers.") && md.contains("### `add`") && md.contains("Adds two numbers."), "{}", md);
    assert!(html.contains("<h3 id=\"add\">") && html.contains("</html>"), "{}", html);
}


// Additional coverage imported from the Claude stdlib implementation.
#[test]
fn named_time_zones() {
    let out = run("main:\n    print(date_format(1793449845, \"%F %T %Z %:z\", \"Europe/Ljubljana\"))\n    print(date_format(1782900000, \"%F %T %Z\", \"Europe/Ljubljana\"))\n    print(date_from_parts(2026, 7, 1, 12, 0, 0, \"Europe/Ljubljana\"))\n    print(date_format(1782900000, \"%F %T %Z\", \"America/New_York\"))\n    print(tz_offset(1782900000, \"Asia/Kolkata\"))\n    print(tz_is_valid(\"Mars/Base\"))\n    print(date_from_parts(2026, 10, 25, 2, 30, 0, \"Europe/Ljubljana\"))\n");
    assert_eq!(
        out,
        vec![
            "2026-10-31 13:30:45 CET +01:00",
            "2026-07-01 12:00:00 CEST",
            "1782900000",
            "2026-07-01 06:00:00 EDT",
            "330",
            "false",
            "1792888200",
        ]
    );
}

#[test]
fn pool_tasks_scopes_and_parallel_map() {
    let source = "fn square(x: num) -> num:\n    return x * x\n\nmain:\n    a = pool_spawn(\"square\", [6])\n    print(task_join(a))\n    s = scope_new()\n    scope_spawn(s, \"square\", [2])\n    scope_spawn(s, \"square\", [3])\n    print(scope_join(s))\n    print(parallel_map(\"square\", [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]))\n";
    assert_eq!(run(source), vec!["36", "[4, 9]", "[1, 4, 9, 16, 25, 36, 49, 64, 81, 100]"]);
}

#[test]
fn a_failing_scope_child_fails_the_join_after_all_children_finish() {
    let source = "fn square(x: num) -> num:\n    return x * x\n\nfn boom(x: num) -> num:\n    test_fail(\"boom\")\n    return x\n\nmain:\n    s = scope_new()\n    scope_spawn(s, \"boom\", [1])\n    scope_spawn(s, \"square\", [2])\n    print(scope_join(s))\n";
    let output = run_with(&[], source);
    assert!(!output.status.success());
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("scope child 0 failed") && text.contains("boom"), "{}", text);
}

#[test]
fn concurrent_async_is_opt_in() {
    let source = "async fn double(x: num) -> num:\n    return x * 2\n\nmain:\n    a = double(4)\n    b = double(5)\n    ra = await a\n    rb = await b\n    print(ra + rb)\n";
    let eager = run_with(&[], source);
    let concurrent = run_with(&["--concurrent-async"], source);
    assert!(eager.status.success() && concurrent.status.success());
    assert_eq!(lines(&eager), vec!["18"]);
    assert_eq!(lines(&concurrent), vec!["18"]);
}

#[test]
fn http_server_and_client_in_one_program() {
    let source = "fn serve(listener: num, requests: num):\n    served = 0\n    while served < requests:\n        request = http_accept(listener)\n        conn = map_get_or(request, \"conn\", 0)\n        path = map_get_or(request, \"path\", \"/\")\n        http_respond(conn, 200, hashmap(\"X-Test\", \"1\"), format(\"hello from {0}\", [path]))\n        served = served + 1\n\nmain:\n    listener = http_listen(\"127.0.0.1\", 0)\n    port = tcp_local_port(listener)\n    server = task_spawn(\"serve\", [listener, 2])\n    first = http_get(format(\"http://127.0.0.1:{0}/one\", [port]))\n    second = http_get(format(\"http://127.0.0.1:{0}/two\", [port]))\n    print(map_get_or(first, \"body\", \"\"))\n    print(map_get_or(second, \"body\", \"\"))\n    done = task_join(server)\n";
    assert_eq!(run(source), vec!["hello from /one", "hello from /two"]);
}
