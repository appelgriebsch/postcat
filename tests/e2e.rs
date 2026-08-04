//! End-to-end tests: real keystrokes and clicks in, real HTTP over a socket,
//! real rendered frames out. Nothing here is mocked — a passing run means the
//! app genuinely works, not that its parts agree with each other.

mod common;

use std::time::Duration;

use common::{Harness, TestServer};
use postcat::app::{EditTarget, Focus, ReqTab, RespTab, SideTab};
use postcat::model::{AuthType, BodyType, Method};
use postcat::theme;

const SHORT: Duration = Duration::from_secs(5);
const LONG: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Layout and navigation
// ---------------------------------------------------------------------------

#[test]
fn startup_renders_every_pane() {
    let h = Harness::new();
    let screen = h.screen();
    for expected in ["postcat", "Library", "Request", "Response", "Query", "NORMAL"] {
        assert!(screen.contains(expected), "missing {expected:?} in:\n{screen}");
    }
    assert!(h.contains("ready when you are"), "empty response placeholder");
    assert!(h.contains("nothing saved yet"), "empty library placeholder");
}

#[test]
fn tab_cycles_focus_in_order() {
    let mut h = Harness::new();
    assert_eq!(h.app.focus, Focus::Url);
    h.tab();
    assert_eq!(h.app.focus, Focus::ReqContent);
    h.tab();
    assert_eq!(h.app.focus, Focus::Response);
    h.tab();
    assert_eq!(h.app.focus, Focus::Sidebar, "wraps to the sidebar");
    h.tab();
    assert_eq!(h.app.focus, Focus::Url);
}

#[test]
fn number_keys_jump_to_panes() {
    let mut h = Harness::new();
    h.press('3');
    assert_eq!(h.app.focus, Focus::ReqContent);
    h.press('4');
    assert_eq!(h.app.focus, Focus::Response);
    h.press('1');
    assert_eq!(h.app.focus, Focus::Sidebar);
    h.press('2');
    assert_eq!(h.app.focus, Focus::Url);
}

#[test]
fn sidebar_can_be_hidden_and_restored() {
    let mut h = Harness::new();
    assert!(h.contains("Library"));
    h.ctrl('b');
    assert!(!h.contains("Library"), "sidebar hidden:\n{}", h.screen());
    h.ctrl('b');
    assert!(h.contains("Library"), "sidebar restored");
}

#[test]
fn help_overlay_opens_and_closes() {
    let mut h = Harness::new();
    h.press('?');
    assert!(h.contains("Panes"), "help sections render:\n{}", h.screen());
    assert!(h.contains("cycle method"));
    h.esc();
    assert!(!h.contains("Panes"), "help closed");
}

#[test]
fn renders_in_a_small_terminal() {
    // 80x24 is the classic floor; panes have minimum-size guards.
    let mut h = Harness::sized(80, 24);
    assert!(h.contains("postcat"));
    h.ctrl('b');
    assert!(h.contains("Response"), "still usable without the sidebar");
}

// ---------------------------------------------------------------------------
// Sending requests
// ---------------------------------------------------------------------------

#[test]
fn sends_a_get_and_renders_the_json_response() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.send_url(&server.url("/echo"));

    assert_eq!(h.status(), Some(200));
    assert_eq!(server.last().method, "GET");
    // Pretty-printed and highlighted in the response pane.
    assert!(h.contains("\"method\": \"GET\""), "body rendered:\n{}", h.screen());
    assert!(h.contains("200 OK"), "status in the meta line");
    assert_eq!(h.app.resp_tab, RespTab::Body);
}

#[test]
fn enter_in_the_url_bar_sends() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.press('i');
    h.type_str(&server.url("/echo"));
    h.enter(); // sends directly from insert mode
    h.await_response();

    assert_eq!(h.status(), Some(200));
    assert_eq!(server.request_count(), 1);
}

#[test]
fn url_without_a_scheme_is_not_sent_as_relative() {
    // Bare host:port is upgraded to https://, which fails against a plain
    // HTTP test server — proving the prefix was applied.
    let server = TestServer::start();
    let mut h = Harness::new();

    h.send_url(&server.hostport());

    assert!(h.app.resp_err.is_some(), "https upgrade should fail here");
    assert_eq!(server.request_count(), 0, "no plaintext request was made");
}

#[test]
fn method_is_sent_and_cycles_with_m() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/echo"));
    h.press('m'); // GET -> POST
    assert_eq!(h.app.draft.method, Method::Post);
    h.press('M'); // back to GET
    assert_eq!(h.app.draft.method, Method::Get);
    h.presses("mmm"); // POST, PUT, PATCH
    assert_eq!(h.app.draft.method, Method::Patch);

    h.press('s');
    h.await_response();
    assert_eq!(server.last().method, "PATCH");
}

#[test]
fn query_params_reach_the_server() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));

    h.press('3'); // request pane, Query tab
    h.enter(); // edit the key cell
    h.type_str("page");
    h.enter(); // move to the value
    h.type_str("2");
    h.enter(); // commit and grow a new row
    h.type_str("q");
    h.enter();
    h.type_str("hello world");
    h.esc();

    h.press('s');
    h.await_response();

    let query = server.last().query();
    assert_eq!(query.get("page").map(String::as_str), Some("2"));
    assert_eq!(query.get("q").map(String::as_str), Some("hello world"));
}

#[test]
fn disabled_rows_are_not_sent() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));

    h.press('3');
    h.enter();
    h.type_str("keep");
    h.enter();
    h.type_str("yes");
    h.enter();
    h.type_str("skip");
    h.enter();
    h.type_str("no");
    h.esc();

    h.press(' '); // toggle the selected (second) row off
    assert!(!h.app.draft.params[1].enabled);

    h.press('s');
    h.await_response();

    let query = server.last().query();
    assert_eq!(query.get("keep").map(String::as_str), Some("yes"));
    assert!(!query.contains_key("skip"), "disabled row was sent: {query:?}");
}

#[test]
fn rows_can_be_deleted() {
    let mut h = Harness::new();
    h.press('3');
    h.enter();
    h.type_str("gone");
    h.esc();
    assert_eq!(h.app.draft.params[0].key, "gone");

    h.press('d');
    assert_eq!(h.app.draft.params.len(), 1, "one blank row remains");
    assert!(h.app.draft.params[0].key.is_empty());
}

#[test]
fn custom_headers_reach_the_server() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));

    h.press('3');
    h.press('l'); // Query -> Headers
    assert_eq!(h.app.req_tab, ReqTab::Headers);
    h.enter();
    h.type_str("X-Trace-Id");
    h.enter();
    h.type_str("abc-123");
    h.esc();

    h.press('s');
    h.await_response();

    assert_eq!(server.last().header("x-trace-id"), Some("abc-123"));
}

#[test]
fn posts_a_json_body() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));
    h.press('m'); // POST

    h.press('3');
    h.presses("ll"); // Query -> Headers -> Body
    assert_eq!(h.app.req_tab, ReqTab::Body);
    h.enter(); // starts editing, body type becomes JSON
    assert_eq!(h.app.draft.body_type, BodyType::Json);
    h.type_str("{\"name\":\"dude\"}");
    h.esc();

    h.press('s');
    h.await_response();

    let req = server.last();
    assert_eq!(req.method, "POST");
    assert_eq!(req.body, "{\"name\":\"dude\"}");
    assert_eq!(req.header("content-type"), Some("application/json"));
}

#[test]
fn format_pretty_prints_the_json_body() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("ll");
    h.enter();
    h.type_str("{\"b\":1,\"a\":[2,3]}");
    h.esc();

    h.press('f');

    assert!(h.contains("\"b\": 1"), "body reflowed:\n{}", h.screen());
    assert!(h.app.draft.body.contains("\n"), "body is multi-line now");
    assert!(h.contains("formatted"), "confirmation toast");
    // Key order is preserved rather than alphabetised.
    let b = h.app.draft.body.find("\"b\"").unwrap();
    let a = h.app.draft.body.find("\"a\"").unwrap();
    assert!(b < a, "original key order kept:\n{}", h.app.draft.body);
}

#[test]
fn format_reports_invalid_json() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("ll");
    h.enter();
    h.type_str("{not json");
    h.esc();

    h.press('f');
    assert!(h.contains("not valid JSON"), "error toast:\n{}", h.screen());
}

#[test]
fn sends_a_form_encoded_body() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));
    h.press('m'); // POST

    h.press('3');
    h.presses("ll"); // Body tab
    h.presses("ttt"); // none -> JSON -> text -> form
    assert_eq!(h.app.draft.body_type, BodyType::Form);
    h.enter();
    h.type_str("user");
    h.enter();
    h.type_str("egoist");
    h.esc();

    h.press('s');
    h.await_response();

    let req = server.last();
    assert_eq!(req.body, "user=egoist");
    assert_eq!(
        req.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
}

#[test]
fn bearer_auth_sets_the_authorization_header() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));

    h.press('3');
    h.presses("lll"); // Query -> Headers -> Body -> Auth
    assert_eq!(h.app.req_tab, ReqTab::Auth);
    h.press('t'); // none -> Bearer
    assert_eq!(h.app.draft.auth.typ, AuthType::Bearer);
    h.enter();
    h.type_str("s3cret-token");
    h.esc();

    h.press('s');
    h.await_response();

    assert_eq!(server.last().header("authorization"), Some("Bearer s3cret-token"));
}

#[test]
fn basic_auth_encodes_credentials() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/echo"));

    h.press('3');
    h.presses("lll");
    h.presses("tt"); // none -> Bearer -> Basic
    assert_eq!(h.app.draft.auth.typ, AuthType::Basic);
    h.enter();
    h.type_str("alice");
    h.enter(); // moves to the password field
    h.type_str("hunter2");
    h.esc();

    // The password renders masked.
    assert!(h.contains("•••••••"), "password masked:\n{}", h.screen());
    assert!(!h.contains("hunter2"), "plaintext password must not render");

    h.press('s');
    h.await_response();

    // base64("alice:hunter2")
    assert_eq!(
        server.last().header("authorization"),
        Some("Basic YWxpY2U6aHVudGVyMg==")
    );
}

#[test]
fn env_vars_substitute_into_url_headers_and_body() {
    let server = TestServer::start();
    let mut h = Harness::new();

    // Define base + token in the environment overlay.
    h.press('e');
    assert!(h.contains("Environment"), "env overlay:\n{}", h.screen());
    h.enter();
    h.type_str("base");
    h.enter();
    h.type_str(&server.url(""));
    h.enter();
    h.type_str("token");
    h.enter();
    h.type_str("from-env");
    h.esc(); // commit the cell
    h.esc(); // close the overlay

    assert!(h.contains("2 env vars"), "header badge:\n{}", h.screen());

    h.set_url("{{base}}/echo?who={{token}}");
    h.press('3');
    h.press('l');
    h.enter();
    h.type_str("X-Token");
    h.enter();
    h.type_str("{{token}}");
    h.esc();

    h.press('s');
    h.await_response();

    let req = server.last();
    assert!(req.path.starts_with("/echo"), "base substituted: {}", req.path);
    assert_eq!(req.query().get("who").map(String::as_str), Some("from-env"));
    assert_eq!(req.header("x-token"), Some("from-env"));
}

// ---------------------------------------------------------------------------
// Response rendering
// ---------------------------------------------------------------------------

#[test]
fn response_headers_tab_lists_headers() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));

    h.press('4'); // response pane
    h.press('l'); // Body -> Headers
    assert_eq!(h.app.resp_tab, RespTab::Headers);
    assert!(h.contains("x-test-server"), "header rows:\n{}", h.screen());
    assert!(h.contains("content-type"));
}

#[test]
fn error_statuses_are_shown_with_their_colour() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/status/404"));

    assert_eq!(h.status(), Some(404));
    assert!(h.contains("404 Not Found"), "status line:\n{}", h.screen());
    assert_eq!(h.fg_at("404 Not Found"), theme::ORANGE, "4xx uses the warn colour");
}

#[test]
fn successful_status_uses_the_ok_colour() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));
    assert_eq!(h.fg_at("200 OK"), theme::GREEN);
}

#[test]
fn connection_failure_renders_an_error_state() {
    let mut h = Harness::new();
    // Port 1 on loopback refuses connections.
    h.send_url("http://127.0.0.1:1/nope");

    assert!(h.app.resp.is_none(), "no response view");
    assert!(h.contains("request failed"), "error panel:\n{}", h.screen());
    assert!(
        h.app.resp_err.as_deref().unwrap_or("").contains("connection failed"),
        "message: {:?}",
        h.app.resp_err
    );
}

#[test]
fn sending_with_no_url_warns_instead_of_firing() {
    let mut h = Harness::new();
    h.press('s');
    assert!(h.contains("no URL"), "toast:\n{}", h.screen());
    assert!(!h.app.loading);
}

#[test]
fn response_scrolls_and_wraps() {
    let server = TestServer::start();
    let mut h = Harness::new();
    // 40 events give a body taller than the pane.
    h.set_url(&server.url("/sse?n=40&ms=1"));
    h.press('s');
    h.expect_text("tick 39", LONG);

    h.press('4');
    h.press('g'); // top
    assert_eq!(h.app.resp_scroll, 0);
    assert!(h.contains("tick 0"), "top of the body:\n{}", h.screen());

    h.press('G'); // bottom
    assert!(h.app.resp_scroll > 0, "scrolled to the tail");
    assert!(h.contains("tick 39"));

    h.press('k');
    let after_up = h.app.resp_scroll;
    h.press('j');
    assert_eq!(h.app.resp_scroll, after_up + 1, "j/k step one line");

    assert!(h.app.wrap);
    h.press('w');
    assert!(!h.app.wrap, "wrap toggles off");
    assert!(h.contains("nowrap"), "meta line shows the mode");
}

// ---------------------------------------------------------------------------
// Streaming (SSE)
// ---------------------------------------------------------------------------

#[test]
fn sse_body_renders_while_the_stream_is_open() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/sse?n=25&ms=40"));
    h.press('s');

    // Early events must be on screen well before the stream ends.
    h.expect_text("tick 2", SHORT);
    assert!(h.app.streaming, "still streaming when early events rendered");
    assert!(!h.contains("tick 24"), "later events have not arrived yet");
    assert!(h.contains("streaming"), "live indicator:\n{}", h.screen());
    assert_eq!(h.status(), Some(200), "headers surfaced before the body finished");

    // And it completes on its own.
    h.expect_text("tick 24", LONG);
    let done = h.wait_until(LONG, |h| !h.app.streaming);
    assert!(done, "stream finished");
    assert!(!h.contains("streaming"), "indicator cleared:\n{}", h.screen());
}

#[test]
fn sse_view_follows_the_tail_then_pins_when_scrolled() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/sse?n=30&ms=30"));
    h.press('s');
    h.expect_text("tick 3", SHORT);

    // Following: the newest event stays visible as the body grows.
    h.expect_text("tick 12", LONG);
    assert!(h.app.follow, "still following");

    // Scrolling up pins the view.
    h.press('4');
    h.press('g');
    assert!(!h.app.follow, "manual scroll stops the follow");
    let pinned = h.app.resp_scroll;
    h.settle(Duration::from_millis(300));
    assert_eq!(h.app.resp_scroll, pinned, "view stayed put while chunks arrived");

    // G re-arms following.
    h.press('G');
    assert!(h.app.follow);
    h.wait_until(LONG, |h| !h.app.streaming);
    assert!(h.contains("tick 29"), "tail visible at the end:\n{}", h.screen());
}

#[test]
fn cancelling_a_stream_keeps_what_arrived() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/sse?n=200&ms=30"));
    h.press('s');
    h.expect_text("tick 1", SHORT);

    h.esc(); // stop the stream
    assert!(!h.app.streaming && !h.app.loading, "stopped");
    assert!(h.contains("cancelled"), "toast:\n{}", h.screen());
    assert!(h.contains("tick 0"), "partial body kept:\n{}", h.screen());

    // Events already queued by the aborted task must not leak in afterwards.
    let seen = h.body().len();
    h.settle(Duration::from_millis(400));
    assert_eq!(h.body().len(), seen, "no chunks landed after the cancel");
}

#[test]
fn a_second_send_replaces_the_first_stream() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/sse?n=200&ms=30"));
    h.press('s');
    h.expect_text("tick 1", SHORT);

    // Re-send while the first stream is live.
    h.set_url(&server.url("/echo"));
    h.press('s');
    h.await_response();

    assert!(!h.app.streaming, "the new response is not a stream");
    assert!(h.contains("\"path\": \"/echo\""), "second response rendered:\n{}", h.screen());
    assert!(!h.contains("tick 5"), "no chunks from the abandoned stream");
}

// ---------------------------------------------------------------------------
// Library: saving, history, renaming, deleting
// ---------------------------------------------------------------------------

#[test]
fn saves_a_request_and_reopens_it() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.set_url(&server.url("/echo?saved=1"));
    h.press('m'); // POST
    h.ctrl('s');
    assert!(h.contains("Save request as"), "prompt:\n{}", h.screen());
    h.type_str("my request");
    h.enter();

    assert_eq!(h.app.saved.len(), 1);
    assert!(h.contains("my request"), "listed in the sidebar:\n{}", h.screen());

    // Blank the editor, then reopen from the library.
    h.press('n');
    assert!(h.app.draft.url.is_empty());
    assert_eq!(h.app.draft.method, Method::Get);

    h.press('1'); // sidebar
    h.enter(); // open the selection
    assert_eq!(h.app.draft.method, Method::Post, "method restored");
    assert!(h.app.draft.url.contains("saved=1"), "url restored");
    assert!(h.contains("Request · my request"), "name in the pane title");
}

#[test]
fn saving_the_same_name_twice_updates_in_place() {
    let mut h = Harness::new();

    h.set_url("http://example.com/one");
    h.ctrl('s');
    h.type_str("dup");
    h.enter();

    h.set_url("http://example.com/two");
    h.ctrl('s');
    h.type_str("dup");
    h.enter();

    assert_eq!(h.app.saved.len(), 1, "no duplicate entry");
    assert!(h.app.saved[0].url.ends_with("/two"), "updated: {}", h.app.saved[0].url);
    assert!(h.contains("updated"), "toast says updated:\n{}", h.screen());
}

#[test]
fn history_records_every_send_newest_first() {
    let server = TestServer::start();
    let mut h = Harness::new();

    h.send_url(&server.url("/echo?first=1"));
    h.send_url(&server.url("/status/404"));

    assert_eq!(h.app.history.len(), 2);
    assert_eq!(h.app.history[0].status, Some(404), "newest first");
    assert_eq!(h.app.history[1].status, Some(200));

    h.press('1');
    h.press('l'); // Saved -> History
    assert_eq!(h.app.side_tab, SideTab::History);
    assert!(h.contains("History 2"), "count:\n{}", h.screen());
    assert!(h.contains("404"), "status shown in the row");
}

#[test]
fn failed_requests_are_recorded_without_a_status() {
    let mut h = Harness::new();
    h.send_url("http://127.0.0.1:1/nope");

    assert_eq!(h.app.history.len(), 1);
    assert_eq!(h.app.history[0].status, None);
    h.press('1');
    h.press('l');
    assert!(h.contains("✗"), "failure marker in the row:\n{}", h.screen());
}

#[test]
fn history_entries_can_be_reopened() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo?replay=yes"));

    h.press('n'); // clear the editor
    assert!(h.app.draft.url.is_empty());

    h.press('1');
    h.press('l'); // History
    h.enter();
    assert!(h.app.draft.url.contains("replay=yes"), "restored: {}", h.app.draft.url);
}

#[test]
fn renames_a_saved_request() {
    let mut h = Harness::new();
    h.set_url("http://example.com/x");
    h.ctrl('s');
    h.type_str("before");
    h.enter();

    h.press('1');
    h.press('r');
    assert!(h.contains("Rename request"), "prompt:\n{}", h.screen());
    h.type_str("after"); // prefill is selected, so typing replaces it
    h.enter();

    assert_eq!(h.app.saved[0].name, "after");
    assert!(h.contains("after"), "sidebar updated:\n{}", h.screen());
}

#[test]
fn deleting_asks_first_and_can_be_declined() {
    let mut h = Harness::new();
    h.set_url("http://example.com/x");
    h.ctrl('s');
    h.type_str("doomed");
    h.enter();

    h.press('1');
    h.press('d');
    assert!(h.contains("Delete"), "confirm dialog:\n{}", h.screen());

    h.press('n'); // decline
    assert_eq!(h.app.saved.len(), 1, "kept");

    h.press('d');
    h.press('y'); // confirm
    assert!(h.app.saved.is_empty(), "deleted");
    assert!(h.contains("nothing saved yet"), "back to the empty state");
}

#[test]
fn new_request_resets_the_editor() {
    let mut h = Harness::new();
    h.set_url("http://example.com/x");
    h.press('3');
    h.enter();
    h.type_str("k");
    h.esc();

    h.press('n');

    assert!(h.app.draft.url.is_empty());
    assert!(h.app.draft.params[0].key.is_empty());
    assert_eq!(h.app.draft.body_type, BodyType::None);
    assert_eq!(h.app.focus, Focus::Url);
}

// ---------------------------------------------------------------------------
// Mouse
// ---------------------------------------------------------------------------

#[test]
fn clicking_request_tabs_switches_them() {
    let mut h = Harness::new();

    h.click_text("Body");
    assert_eq!(h.app.req_tab, ReqTab::Body);
    assert_eq!(h.app.focus, Focus::ReqContent);

    h.click_text("Auth");
    assert_eq!(h.app.req_tab, ReqTab::Auth);

    h.click_text("Query");
    assert_eq!(h.app.req_tab, ReqTab::Query);
}

#[test]
fn clicking_response_tabs_switches_them() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));

    h.click_text("Headers 4"); // the response tab carries a count badge
    assert_eq!(h.app.resp_tab, RespTab::Headers);
    assert_eq!(h.app.focus, Focus::Response);
    assert!(h.contains("x-test-server"));
}

#[test]
fn clicking_sidebar_tabs_switches_them() {
    let mut h = Harness::new();
    h.click_text("History 0");
    assert_eq!(h.app.side_tab, SideTab::History);
    assert_eq!(h.app.focus, Focus::Sidebar);
    h.click_text("Saved 0");
    assert_eq!(h.app.side_tab, SideTab::Saved);
}

#[test]
fn clicking_a_sidebar_row_opens_it_immediately() {
    let mut h = Harness::new();
    h.set_url("http://example.com/clicked");
    h.ctrl('s');
    h.type_str("clickme");
    h.enter();
    h.press('n'); // blank the editor
    assert!(h.app.draft.url.is_empty());

    h.click_text("clickme");

    assert!(h.app.draft.url.contains("clicked"), "opened by a single click");
    assert!(h.contains("opened"), "confirmation toast:\n{}", h.screen());
    assert_eq!(h.app.focus, Focus::Url, "focus moves to the url bar");
}

#[test]
fn clicking_the_url_bar_starts_editing() {
    let mut h = Harness::new();
    h.press('3'); // focus elsewhere first
    let bar = h.app.rects.url;
    h.click(bar.x + 25, bar.y + 1); // inside the URL text, past the method chip
    assert!(h.app.in_insert(), "insert mode:\n{}", h.screen());
    assert_eq!(h.app.focus, Focus::Url);
}

#[test]
fn method_dropdown_opens_and_picks_with_the_mouse() {
    let mut h = Harness::new();

    h.click_text("GET");
    assert!(h.contains("Method"), "dropdown open:\n{}", h.screen());
    assert!(h.contains("OPTIONS"), "all methods listed");

    h.click_text("DELETE");
    assert_eq!(h.app.draft.method, Method::Delete);
    assert!(!h.contains("OPTIONS"), "menu closed:\n{}", h.screen());
}

#[test]
fn method_dropdown_supports_the_keyboard() {
    let mut h = Harness::new();
    h.click_text("GET");

    h.press('j'); // GET -> POST
    h.press('j'); // -> PUT
    h.enter();

    assert_eq!(h.app.draft.method, Method::Put);
    assert!(!h.contains("OPTIONS"), "menu closed");
}

#[test]
fn method_dropdown_dismisses_without_changing() {
    let mut h = Harness::new();
    h.click_text("GET");
    h.esc();
    assert_eq!(h.app.draft.method, Method::Get);

    h.click_text("GET");
    h.click(120, 30); // click far away
    assert_eq!(h.app.draft.method, Method::Get);
    assert!(!h.contains("OPTIONS"), "menu dismissed");
}

#[test]
fn wheel_scrolls_the_response() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/sse?n=40&ms=1"));
    h.press('s');
    h.expect_text("tick 39", LONG);

    h.press('4');
    h.press('g');
    assert_eq!(h.app.resp_scroll, 0);

    let (_, y) = h.find("tick 1");
    h.scroll_down(60, y);
    assert!(h.app.resp_scroll > 0, "wheel scrolled down");
    let after = h.app.resp_scroll;
    h.scroll_up(60, y);
    assert!(h.app.resp_scroll < after, "wheel scrolled back up");
}

#[test]
fn scrollbar_thumb_drags_the_response() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/sse?n=40&ms=1"));
    h.press('s');
    h.expect_text("tick 39", LONG);

    h.press('4');
    h.press('g');
    assert_eq!(h.app.resp_scroll, 0);

    let track = h.app.resp_sb;
    assert!(track.height > 0, "scrollbar is on screen:\n{}", h.screen());
    let (x, top) = (track.x, track.y);
    let bottom = track.y + track.height - 1;

    // Grab the thumb at the top of the track and haul it to the bottom.
    h.click(x, top);
    assert_eq!(h.app.resp_scroll, 0, "grabbing the thumb does not move it");
    h.drag(x, top + track.height / 2);
    let mid = h.app.resp_scroll;
    assert!(mid > 0, "drag scrolled down");
    h.drag(x, bottom);
    assert!(h.app.resp_scroll > mid, "drag reached the tail");
    assert!(h.contains("tick 39"), "last line visible:\n{}", h.screen());
    assert!(!h.app.follow, "dragging pins the view");

    // Back up, then release: further motion is ignored.
    h.drag(x, top);
    assert_eq!(h.app.resp_scroll, 0, "dragged back to the top");
    h.release(x, top);
    h.drag(x, bottom);
    assert_eq!(h.app.resp_scroll, 0, "released thumb ignores the pointer");

    // A click on empty track jumps the view there.
    h.click(x, bottom);
    assert!(h.app.resp_scroll > 0, "clicking the track jumps");
}

// ---------------------------------------------------------------------------
// Editing ergonomics
// ---------------------------------------------------------------------------

#[test]
fn pointer_is_a_text_cursor_only_over_the_active_editor() {
    let mut h = Harness::new();
    let bar = h.app.rects.url;
    let (url_x, url_y) = (bar.x + 25, bar.y + 1);

    // Normal mode: the URL bar is a control, not a text field.
    h.move_mouse(url_x, url_y);
    assert!(!h.app.hover_text, "arrow in normal mode");

    // Insert mode: I-beam over the field being edited...
    h.press('i');
    assert!(h.app.edit_rect.height > 0, "the editor rect was recorded");
    h.move_mouse(url_x, url_y);
    assert!(h.app.hover_text, "text cursor over the active editor");

    // ...and an arrow anywhere else, even while still editing.
    let resp = h.app.rects.response;
    h.move_mouse(resp.x + 10, resp.y + 5);
    assert!(!h.app.hover_text, "arrow away from the editor");

    h.esc();
    h.move_mouse(url_x, url_y);
    assert!(!h.app.hover_text, "arrow again after leaving insert mode");
}

#[test]
fn the_body_editor_is_a_text_target_too() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("ll"); // Body tab
    h.enter(); // start editing
    let ed = h.app.edit_rect;
    assert!(ed.height > 1, "the body textarea rect was recorded");
    h.move_mouse(ed.x + 3, ed.y + 1);
    assert!(h.app.hover_text, "text cursor inside the body editor");

    // The tab strip above it is still a control.
    let (tx, ty) = h.find("Query");
    h.move_mouse(tx, ty);
    assert!(!h.app.hover_text, "arrow over the tabs");
}

#[test]
fn opening_a_row_with_a_key_but_no_value_jumps_to_the_value() {
    let mut h = Harness::new();
    h.press('3');
    h.enter();
    h.type_str("key-only");
    h.esc();

    h.enter(); // the key is filled, so this lands on the value cell
    h.type_str("filled-in");
    h.esc();

    let row = &h.app.draft.params[0];
    assert_eq!(row.key, "key-only", "the key was left alone");
    assert_eq!(row.value, "filled-in");
}

#[test]
fn reopening_a_complete_cell_replaces_what_you_type() {
    let mut h = Harness::new();
    h.press('3');
    h.enter();
    h.type_str("k");
    h.tab();
    h.type_str("v");
    h.esc();

    h.enter(); // both filled → opens the key with its text selected
    h.type_str("replaced");
    h.esc();

    let row = &h.app.draft.params[0];
    assert_eq!(row.key, "replaced", "typing replaced the selection");
    assert_eq!(row.value, "v", "the value survived");
}

#[test]
fn tab_moves_between_key_and_value_while_editing() {
    let mut h = Harness::new();
    h.press('3');
    h.enter();
    h.type_str("k");
    h.tab(); // jump to the value column
    h.type_str("v");
    h.esc();

    assert_eq!(h.app.draft.params[0].key, "k");
    assert_eq!(h.app.draft.params[0].value, "v");
}

#[test]
fn escape_keeps_url_edits_and_ctrl_u_clears_them() {
    let mut h = Harness::new();
    h.press('i');
    h.type_str("http://example.com/keep");
    h.esc();
    assert_eq!(h.app.draft.url, "http://example.com/keep", "esc commits");

    h.press('i');
    h.ctrl('u');
    h.esc();
    assert!(h.app.draft.url.is_empty(), "ctrl+u cleared the line");
}

#[test]
fn pasting_a_multiline_url_flattens_it() {
    let mut h = Harness::new();
    h.press('i');
    h.paste("http://example.com/a\n");
    h.esc();
    assert_eq!(h.app.draft.url, "http://example.com/a", "newline stripped");
}

#[test]
fn insert_mode_is_reflected_in_the_status_bar() {
    let mut h = Harness::new();
    assert!(h.contains("NORMAL"));
    h.press('i');
    assert!(h.contains("INSERT"), "mode chip:\n{}", h.screen());
    h.esc();
    assert!(h.contains("NORMAL"));
}

#[test]
fn typing_a_url_does_not_trigger_normal_mode_shortcuts() {
    // "q" would quit and "s" would send in normal mode; in insert they are text.
    let mut h = Harness::new();
    h.press('i');
    h.type_str("http://example.com/q?s=1");
    h.esc();

    assert!(!h.app.should_quit, "still running");
    assert_eq!(h.app.draft.url, "http://example.com/q?s=1");
}

// ---------------------------------------------------------------------------
// Selecting & copying
// ---------------------------------------------------------------------------

#[test]
fn dragging_the_response_selects_and_copies_what_it_covers() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));

    let (x, y) = h.find("\"method\"");
    let end = x + 7; // the closing quote
    h.select((x, y), (end, y));

    assert_eq!(h.app.last_copy.as_deref(), Some("\"method\""));
    assert_eq!(h.bg_at(x, y), theme::ACCENT, "highlighted:\n{}", h.screen());
    assert_eq!(h.bg_at(end, y), theme::ACCENT, "to the last cell dragged over");
    assert_ne!(h.bg_at(end + 1, y), theme::ACCENT, "and no further");
    assert!(h.contains("copied"), "confirmation toast:\n{}", h.screen());

    // The same span, dragged right to left.
    h.select((end, y), (x, y));
    assert_eq!(h.app.last_copy.as_deref(), Some("\"method\""), "backwards too");
}

#[test]
fn a_selection_spanning_lines_copies_each_of_them() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));

    let (x, y) = h.find("\"method\"");
    h.select((x, y), (x + 7, y + 1));

    let copied = h.app.last_copy.clone().expect("copied something");
    let lines: Vec<&str> = copied.lines().collect();
    assert_eq!(lines.len(), 2, "two rows dragged over: {copied:?}");
    assert!(lines[0].starts_with("\"method\""), "from the press: {copied:?}");
}

#[test]
fn a_press_that_never_moved_drops_the_selection() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));

    let (x, y) = h.find("\"method\"");
    h.select((x, y), (x + 7, y));
    assert!(h.app.resp_sel.is_some(), "selected");

    h.click(x, y);
    h.release(x, y);
    assert!(h.app.resp_sel.is_none(), "a plain click clears it");
    assert_ne!(h.bg_at(x, y), theme::ACCENT, "highlight gone:\n{}", h.screen());
}

#[test]
fn scrolling_retires_the_response_selection() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.set_url(&server.url("/sse?n=40&ms=1"));
    h.press('s');
    h.expect_text("tick 39", LONG);

    h.press('4');
    let (x, y) = h.find("tick 3");
    h.select((x, y), (x + 5, y));
    assert!(h.app.resp_sel.is_some());

    // The selection is pinned to screen cells, so it cannot outlive the view
    // it was made in.
    h.press('k');
    assert!(h.app.resp_sel.is_none(), "dropped rather than left pointing elsewhere");
    assert!(h.app.resp_sel_text.is_empty());
}

#[test]
fn y_copies_the_selection_or_the_whole_body() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));
    h.press('4');

    h.press('y');
    assert_eq!(
        h.app.last_copy.as_deref(),
        Some(h.body().as_str()),
        "with nothing selected: the raw body, not just what fits on screen"
    );

    let (x, y) = h.find("\"method\"");
    h.select((x, y), (x + 7, y));
    h.press('y');
    assert_eq!(h.app.last_copy.as_deref(), Some("\"method\""), "the selection wins");
}

#[test]
fn y_copies_the_header_list_on_the_headers_tab() {
    let server = TestServer::start();
    let mut h = Harness::new();
    h.send_url(&server.url("/echo"));
    h.click_text("Headers 4");

    h.press('y');
    let copied = h.app.last_copy.clone().expect("copied something");
    assert!(copied.contains("x-test-server: postcat"), "as key: value pairs: {copied:?}");
}

#[test]
fn clicking_the_url_bar_puts_the_caret_where_it_lands() {
    let mut h = Harness::new();
    h.set_url("http://example.com/path");

    let (x, y) = h.find("http://example.com/path");
    h.click(x + 4, y);
    h.release(x + 4, y);

    assert_eq!(h.app.url_ta.cursor(), (0, 4), "not parked at the end");
    h.type_str("s");
    assert_eq!(h.app.url_ta.lines()[0], "https://example.com/path");
}

#[test]
fn clicking_a_scrolled_url_lands_on_the_character_under_the_pointer() {
    let mut h = Harness::new();
    // Longer than the bar, so the field is scrolled sideways and the caret has
    // to be worked out from the rendered text rather than the raw string.
    let url = format!("http://example.com/{}/tail-marker", "x".repeat(120));
    h.set_url(&url);

    let (x, y) = h.find("tail-marker");
    h.click(x, y);
    h.release(x, y);

    let col = h.app.url_ta.cursor().1;
    let under: String = url.chars().skip(col).take(11).collect();
    assert_eq!(under, "tail-marker", "caret landed elsewhere (col {col})");
}

#[test]
fn dragging_in_the_url_bar_selects_and_copies() {
    let mut h = Harness::new();
    h.set_url("http://example.com/path");

    let (x, y) = h.find("http://example.com/path");
    let from = x + "http://".chars().count() as u16;
    h.select((from, y), (from + 7, y));

    assert_eq!(h.app.url_ta.selection_range(), Some(((0, 7), (0, 14))), "“example”");
    assert_eq!(h.app.last_copy.as_deref(), Some("example"));

    // Still a live selection: typing replaces it, as it would in any field.
    h.type_str("other");
    assert_eq!(h.app.url_ta.lines()[0], "http://other.com/path");
}

#[test]
fn clicking_the_json_body_starts_editing_where_it_lands() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("ll"); // Body tab
    h.enter();
    h.type_str("{\"a\": 1}");
    h.esc();
    h.press('f'); // pretty-printed, so the line numbers are in play

    let (x, y) = h.find("\"a\"");
    h.click(x + 1, y);
    h.release(x + 1, y);

    assert_eq!(h.app.edit, EditTarget::Body, "a click opens the editor");
    assert_eq!(h.app.body_ta.cursor(), (1, 3), "past the line-number gutter");
}

#[test]
fn dragging_in_the_json_body_selects_and_copies() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("ll");
    h.enter();
    h.type_str("{\"a\": 1}");
    h.esc();
    h.press('f');

    let (x, y) = h.find("\"a\"");
    h.select((x, y), (x + 3, y));

    assert_eq!(h.app.last_copy.as_deref(), Some("\"a\""));
    assert_eq!(h.app.body_ta.selection_range(), Some(((1, 2), (1, 5))));
}

#[test]
fn dragging_a_password_field_never_copies_it() {
    let mut h = Harness::new();
    h.press('3');
    h.presses("lll"); // Auth tab
    h.press('t');
    h.press('t'); // None -> Bearer -> Basic
    h.enter();
    h.type_str("user");
    h.enter(); // on to the password
    h.type_str("hunter2");

    let field = h.app.edit_rect;
    h.select((field.x, field.y), (field.x + 6, field.y));

    assert_eq!(h.app.last_copy, None, "a masked field stays out of the clipboard");
    assert!(
        h.app.auth_ta.as_ref().unwrap().selection_range().is_some(),
        "but it still selects, so you can type over it"
    );
}

// ---------------------------------------------------------------------------
// Workspace persistence
// ---------------------------------------------------------------------------

#[test]
fn workspace_round_trips_through_a_file() {
    use postcat::model::{Persisted, RequestModel};

    let dir = std::env::temp_dir().join(format!("postcat-test-{}", std::process::id()));
    let path = dir.join("state.json");
    let _ = std::fs::remove_dir_all(&dir);

    let saved = RequestModel {
        name: "persisted".into(),
        url: "http://example.com/p".into(),
        method: Method::Delete,
        ..Default::default()
    };

    Persisted {
        saved: vec![saved],
        history: Vec::new(),
        env: Vec::new(),
        draft: None,
    }
    .save(Some(&path));

    let loaded = Persisted::load(Some(&path));
    assert_eq!(loaded.saved.len(), 1);
    assert_eq!(loaded.saved[0].name, "persisted");
    assert_eq!(loaded.saved[0].method, Method::Delete);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ephemeral_app_never_writes_to_disk() {
    let mut h = Harness::new();
    assert!(h.app.state_path.is_none(), "tests must not touch the real workspace");
    h.set_url("http://example.com/x");
    h.ctrl('s');
    h.type_str("nope");
    h.enter(); // triggers persist(), which must be a no-op here
    assert_eq!(h.app.saved.len(), 1, "state still lives in memory");
}

#[test]
fn quitting_sets_the_exit_flag() {
    let mut h = Harness::new();
    assert!(!h.app.should_quit);
    h.press('q');
    assert!(h.app.should_quit);
}

