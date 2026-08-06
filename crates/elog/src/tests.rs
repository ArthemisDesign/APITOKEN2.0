use crate::scrub_secrets;

#[test]
fn bearer_token_is_masked() {
    assert_eq!(
        scrub_secrets("upstream refused Bearer sk-ant-api03-abcDEF123 token"),
        "upstream refused Bearer *** token"
    );
    assert_eq!(
        scrub_secrets("auth header: bearer x.y.z end"),
        "auth header: bearer *** end"
    );
}

#[test]
fn sk_tokens_are_masked_anywhere() {
    assert_eq!(
        scrub_secrets("issued sk-pool-0123456789abcdef on request"),
        "issued *** on request"
    );
    assert_eq!(
        scrub_secrets("key=sk-ant-api03-xyz,"),
        "key=***,"
    );
    assert_eq!(scrub_secrets("sk-openkeys-deadbeef"), "***");
}

#[test]
fn google_oauth_token_is_masked() {
    assert_eq!(
        scrub_secrets("oauth ya29.a0AfH6SMABC123token expired"),
        "oauth *** expired"
    );
}

#[test]
fn key_headers_are_masked() {
    assert_eq!(
        scrub_secrets("request had x-api-key: secret-value and body"),
        "request had x-api-key: *** and body"
    );
    assert_eq!(
        scrub_secrets("x-goog-api-key: AAAAbbbbCCCC done"),
        "x-goog-api-key: *** done"
    );
}

#[test]
fn plain_text_survives_unchanged() {
    let plain = "subs admin query failed: connection refused";
    assert_eq!(scrub_secrets(plain), plain);
    let task_word = "task-ant design, beaker-ya29 talk";
    assert_eq!(scrub_secrets(task_word), task_word);
}

#[test]
fn masked_token_takes_the_whole_run() {
    assert_eq!(
        scrub_secrets("Bearer sk-ant-api03-0123456789abcdef:quoted"),
        "Bearer ***:quoted"
    );
    assert_eq!(
        scrub_secrets("Bearer abc } json"),
        "Bearer *** } json"
    );
}
